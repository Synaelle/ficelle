// SPDX-License-Identifier: AGPL-3.0-or-later

//! Sync with a server: signing in, and a thread that runs sync rounds.
//!
//! The thread wakes when something was written (every command kicks it),
//! every minute to pull what other devices sent, and after a failure once
//! its backoff wait is over. It holds the vault only for the short steps
//! of a round, never while waiting on the network. The ui hears about it
//! through two events: `sync-status` with a [`SyncStatus`], and
//! `sync-changed` when changes from elsewhere were taken in.

pub mod catalog;
pub mod commands;
pub mod extensions;
pub mod feedback;
pub mod files;
mod http;
mod live;
pub mod sharing;
pub(crate) mod sign_in;

use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use app_core::store::{Store, Tokens};
use app_core::sync::engine::{self, Access, Backoff, Remote, RemoteError, SyncError};
use app_core::sync::files::{FileError, FilePart};
use app_protocol::account::{RefreshRequest, Session};
use app_protocol::e2ee::AccountKind;
use app_protocol::sync::{PullAnswer, PushAnswer, PushRequest};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use self::http::{Api, HttpError};
use crate::state::AppState;

/// Ask the server which documents to compact this often.
const COMPACT_EVERY: Duration = Duration::from_hours(6);
/// Pull at least this often while nothing else wakes the thread.
const PULL_EVERY: Duration = Duration::from_secs(60);
/// Writes come in bursts while typing: wait for this much quiet first, so
/// a note being written goes up every few seconds, not at every pause.
const SETTLE: Duration = Duration::from_secs(3);
/// While connected live: this account's other devices may have the note
/// open (no presence tells), they see an edit a few seconds after it is
/// typed (the editor saves after a second of quiet).
const SETTLE_LIVE: Duration = Duration::from_millis(1500);
/// While someone else is known to have the open note of a shared space
/// open: co-editing, an edit shows about a second after it is typed.
const SETTLE_COEDITING: Duration = Duration::from_millis(300);

/// Where sync stands, for the indicator and the settings.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    /// `off`, `signedOut`, `syncing`, `offline`, `error`, `pending` or
    /// `synced`.
    pub state: &'static str,
    pub server: Option<String>,
    pub username: Option<String>,
    pub e2ee: bool,
    /// Local changes not yet on the server.
    pub pending: i64,
    pub last_sync_at: Option<i64>,
    pub error: Option<String>,
    /// Changes from the server that could not be read.
    pub rejected: i64,
    /// Connected live: other devices' changes arrive within a second.
    pub live: bool,
    /// The server's admin allows co-editing.
    pub coediting: bool,
}

impl SyncStatus {
    fn off() -> Self {
        Self {
            state: "off",
            server: None,
            username: None,
            e2ee: false,
            pending: 0,
            last_sync_at: None,
            error: None,
            rejected: 0,
            live: false,
            coediting: true,
        }
    }
}

/// How the last round ended, kept in memory.
#[derive(Debug, Clone, Default)]
enum Last {
    #[default]
    None,
    Ok,
    Offline(String),
    Failed(String),
}

#[derive(Default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent flags of the sync threads, read under one lock"
)]
struct Shared {
    kicked: bool,
    /// Pull even when nothing waits to be sent.
    force: bool,
    /// Last status sent to the ui, so reads do not resend it.
    sent: Option<SyncStatus>,
    running: bool,
    last: Last,
    /// No round before this, after a failure.
    not_before: Option<Instant>,
    /// The live socket is up.
    live: bool,
    /// The server's co-editing setting, as last heard.
    coediting: Option<bool>,
    /// When documents were last offered for compaction.
    compacted: Option<Instant>,
    /// The note of a shared space open in the ui, said on the live socket.
    viewing: Option<(String, String)>,
    /// Per shared space, the note each of its viewers has open, from the
    /// live socket: two on the open note means someone may be co-editing.
    presence: std::collections::HashMap<String, Vec<String>>,
    /// The account's files policy and usage, as the last round heard.
    files: Option<app_protocol::files::FilesInfo>,
    /// Files up to this size are fetched by the rounds, larger ones when
    /// opened. `None` until the ui says, then the default.
    auto_fetch: Option<u64>,
    /// A round of the notes ended: the files thread makes a pass.
    files_wanted: bool,
    /// When removed files were last cleaned up here.
    cleaned: Option<Instant>,
}

/// The sync thread's handle, kept in the app state.
#[derive(Clone, Default)]
pub struct Sync {
    shared: Arc<(Mutex<Shared>, Condvar)>,
    /// Held while tokens are refreshed: the sync and live threads must
    /// never both trade the same refresh token, the server would take the
    /// second use for a stolen token and sign the device out.
    refreshing: Arc<Mutex<()>>,
}

impl Sync {
    fn lock(&self) -> std::sync::MutexGuard<'_, Shared> {
        self.shared
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The account's files policy as the last round heard, `None` without
    /// a server that keeps files.
    pub fn files_policy(&self) -> Option<app_protocol::files::FilesPolicy> {
        self.lock().files.as_ref().map(|info| info.policy.clone())
    }

    /// Asks for a round soon. Cheap, every command calls it.
    pub fn kick(&self) {
        self.lock().kicked = true;
        self.shared.1.notify_all();
    }

    /// Asks for a round now, even right after a failure.
    pub fn now(&self) {
        let mut shared = self.lock();
        shared.kicked = true;
        shared.force = true;
        shared.not_before = None;
        self.shared.1.notify_all();
    }

    /// Starts the thread.
    pub fn start(&self, app: AppHandle) {
        let this = self.clone();
        let live_app = app.clone();
        std::thread::Builder::new()
            .name("sync".into())
            .spawn(move || this.run(&app))
            .expect("the sync thread starts");
        let this = self.clone();
        let files_app = live_app.clone();
        std::thread::Builder::new()
            .name("sync-live".into())
            .spawn(move || live::run(&this, &live_app))
            .expect("the live sync thread starts");
        let this = self.clone();
        std::thread::Builder::new()
            .name("sync-files".into())
            .spawn(move || files::run(&this, &files_app))
            .expect("the files thread starts");
    }

    /// Asks the files thread for a pass, after a round of the notes.
    fn files_due(&self) {
        self.lock().files_wanted = true;
        self.shared.1.notify_all();
    }

    /// The live socket went up or down.
    fn set_live(&self, up: bool) {
        self.lock().live = up;
    }

    /// The note of a shared space open in the ui, none when there is none.
    pub fn set_viewing(&self, viewing: Option<(String, String)>) {
        self.lock().viewing = viewing;
    }

    /// The open note, and whether the server allows co-editing: said
    /// again when that is turned back on, the server forgot it.
    fn viewing(&self) -> (Option<(String, String)>, bool) {
        let shared = self.lock();
        (shared.viewing.clone(), shared.coediting.unwrap_or(true))
    }

    /// The server's co-editing setting, from the live socket.
    fn set_coediting(&self, on: bool) {
        self.lock().coediting = Some(on);
    }

    /// Who has which note of a shared space open, from the live socket.
    fn set_presence(&self, space: String, notes: Vec<String>) {
        self.lock().presence.insert(space, notes);
    }

    /// The open note is open somewhere else too (another person, or another
    /// of this account's devices), and the live socket carries its edits.
    fn coedited(&self) -> bool {
        let shared = self.lock();
        let Some((space, note)) = &shared.viewing else {
            return false;
        };
        shared.live
            && shared.coediting.unwrap_or(true)
            && shared
                .presence
                .get(space)
                .is_some_and(|notes| notes.iter().filter(|n| *n == note).count() >= 2)
    }

    fn run(&self, app: &AppHandle) {
        let mut backoff = Backoff::default();
        let mut last_pull = Instant::now();
        loop {
            // wait for a kick, the next pull, or the end of a backoff
            {
                let mut shared = self.lock();
                loop {
                    let now = Instant::now();
                    let waited = shared.not_before.is_none_or(|t| now >= t);
                    let pull_due = now.duration_since(last_pull) >= PULL_EVERY;
                    if waited && (shared.kicked || pull_due) {
                        break;
                    }
                    let until = shared
                        .not_before
                        .filter(|t| *t > now)
                        .unwrap_or(last_pull + PULL_EVERY);
                    let timeout = until
                        .saturating_duration_since(now)
                        .max(Duration::from_millis(50));
                    shared = self
                        .shared
                        .1
                        .wait_timeout(shared, timeout)
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .0;
                }
                shared.kicked = false;
            }
            // let a burst of writes end, then decide whether there is work
            let settle = if self.coedited() {
                SETTLE_COEDITING
            } else if self.lock().live {
                SETTLE_LIVE
            } else {
                SETTLE
            };
            std::thread::sleep(settle);
            let forced = {
                let mut shared = self.lock();
                shared.kicked = false;
                std::mem::take(&mut shared.force)
            };
            let state = app.state::<AppState>();
            let due = forced || Instant::now().duration_since(last_pull) >= PULL_EVERY;
            let ready = state.quietly(|s| {
                let link = s.store.sync_link()?;
                let pending = s.store.sync_counts()?.pending;
                Ok(link.filter(|l| l.tokens.is_some()).map(|_| pending))
            });
            let Ok(Some(pending)) = ready else {
                // locked, not linked or signed out
                emit_status(app);
                continue;
            };
            if pending == 0 && !due {
                emit_status(app);
                continue;
            }
            self.lock().running = true;
            emit_status(app);
            let outcome = round(&state);
            last_pull = Instant::now();
            {
                let mut shared = self.lock();
                shared.running = false;
                match &outcome {
                    Ok(_) => {
                        backoff.succeeded();
                        shared.last = Last::Ok;
                        shared.not_before = None;
                    }
                    Err(err) => {
                        let wait = backoff.failed(random());
                        shared.not_before = Some(Instant::now() + wait);
                        shared.last = match err {
                            SyncError::Remote(RemoteError::Offline(why)) => {
                                Last::Offline(why.clone())
                            }
                            other => Last::Failed(other.to_string()),
                        };
                    }
                }
            }
            let message = match &outcome {
                Ok(_) | Err(SyncError::Remote(RemoteError::Offline(_))) => None,
                Err(err) => Some(err.to_string()),
            };
            let _ = state.quietly(|s| s.store.sync_finished(message.as_deref()));
            if let Ok(round) = &outcome
                && round.taken.applied > 0
            {
                let _ = app.emit("sync-changed", ());
            }
            emit_status(app);
        }
    }

    fn status(&self, state: &AppState) -> SyncStatus {
        let link = state.quietly(|s| {
            let Some(link) = s.store.sync_link()? else {
                return Ok(None);
            };
            Ok(Some((
                link,
                s.store.sync_counts()?.pending,
                s.store.sync_rejected()?,
            )))
        });
        let Ok(Some((link, pending, rejected))) = link else {
            return SyncStatus::off();
        };
        let shared = self.lock();
        let state = if link.tokens.is_none() {
            "signedOut"
        } else if shared.running
            || (matches!(shared.last, Last::None) && link.last_sync_at.is_none())
        {
            // just linked, the first round is on its way
            "syncing"
        } else {
            match &shared.last {
                Last::Offline(_) => "offline",
                Last::Failed(_) => "error",
                _ if pending > 0 => "pending",
                _ => "synced",
            }
        };
        let error = match &shared.last {
            Last::Offline(why) | Last::Failed(why) => Some(why.clone()),
            _ => link.last_error.clone(),
        };
        SyncStatus {
            state,
            server: Some(link.server),
            username: Some(link.username),
            e2ee: link.kind == AccountKind::E2ee,
            pending,
            last_sync_at: link.last_sync_at,
            error,
            rejected,
            live: shared.live,
            coediting: shared.coediting.unwrap_or(true),
        }
    }

    /// Forgets the last outcome, for a new link.
    fn reset(&self) {
        let mut shared = self.lock();
        shared.last = Last::None;
        shared.not_before = None;
        shared.coediting = None;
    }
}

fn emit_status(app: &AppHandle) {
    let state = app.state::<AppState>();
    let status = state.sync.status(&state);
    let mut shared = state.sync.lock();
    if shared.sent.as_ref() != Some(&status) {
        shared.sent = Some(status.clone());
        drop(shared);
        let _ = app.emit("sync-status", status);
    }
}

fn random() -> u64 {
    // only spreads retries apart, it needs no secrecy
    use std::hash::BuildHasher;
    std::hash::RandomState::new().hash_one(Instant::now())
}

/// The vault as the engine sees it: short turns with the open session.
struct Vault<'a>(&'a AppState);

impl Access for Vault<'_> {
    fn with<T>(&self, f: impl FnOnce(&mut Store) -> app_core::Result<T>) -> app_core::Result<T> {
        self.0
            .quietly(|s| f(&mut s.store))
            .map_err(|_| app_core::Error::Invalid("the vault is locked".into()))
    }
}

/// The server of the linked account.
fn wire(state: &AppState) -> Result<(Wire<'_>, AccountKind), SyncError> {
    let link = Vault(state)
        .with(|s| s.sync_link())?
        .ok_or_else(|| app_core::Error::Invalid("not linked".into()))?;
    let tokens = link.tokens.ok_or(RemoteError::SignedOut)?;
    Ok((
        Wire {
            api: Api::new(&link.server),
            tokens,
            state,
        },
        link.kind,
    ))
}

fn round(state: &AppState) -> Result<engine::Round, SyncError> {
    let (mut wire, kind) = wire(state)?;
    if kind == AccountKind::E2ee {
        sharing::ensure_key_pair(state, &mut wire)?;
    }
    let done = engine::run(&Vault(state), &mut wire)?;
    // now and then, documents the server holds many changes of go as one
    // whole document, the server empties the changes it replaces
    let due = {
        let mut shared = state.sync.lock();
        let due = shared
            .compacted
            .is_none_or(|t| t.elapsed() >= COMPACT_EVERY);
        if due {
            shared.compacted = Some(Instant::now());
        }
        due
    };
    if due && engine::compact(&Vault(state), &mut wire, engine::COMPACT_AFTER)? > 0 {
        engine::run(&Vault(state), &mut wire)?;
    }
    // files go on their own thread: a large one never holds up the notes
    state.sync.files_due();
    Ok(done)
}

/// The server, with the tokens refreshed when they run out.
struct Wire<'a> {
    api: Api,
    tokens: Tokens,
    state: &'a AppState,
}

fn remote_error(err: HttpError) -> RemoteError {
    match err {
        // a certificate that does not check out: nothing is sent, the sync waits
        HttpError::Offline(why) | HttpError::Certificate(why) => RemoteError::Offline(why),
        HttpError::Api { status: 401, .. } => RemoteError::SignedOut,
        // the server is there but struggling, or asks to slow down: try
        // again later like when it cannot be reached
        HttpError::Api { status, body } if status >= 500 || status == 429 => {
            RemoteError::Offline(format!("{status} {}", body.error))
        }
        other @ HttpError::Api { .. } => RemoteError::Refused(other.to_string()),
    }
}

fn tokens_of(session: &Session) -> Tokens {
    Tokens {
        access: session.access_token.clone(),
        access_expires_at: session.access_expires_at,
        refresh: session.refresh_token.clone(),
        refresh_expires_at: session.refresh_expires_at,
    }
}

/// New tokens in place of `worn`, the access token that stopped working
/// or is about to. One refresh at a time: when another thread already
/// refreshed, its tokens are taken from the vault instead.
fn refresh(state: &AppState, api: &Api, worn: &str) -> Result<Tokens, RemoteError> {
    let _one_at_a_time = state
        .sync
        .refreshing
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let stored = state
        .quietly(|s| s.store.sync_link())
        .map_err(|_| RemoteError::Offline("the vault is locked".into()))?
        .and_then(|link| link.tokens)
        .ok_or(RemoteError::SignedOut)?;
    if stored.access != worn {
        return Ok(stored);
    }
    let answer = api.post(
        "/auth/refresh",
        None,
        &RefreshRequest {
            refresh_token: stored.refresh.clone(),
        },
    );
    match answer.map_err(remote_error).and_then(|a| {
        a.parse::<Session>()
            .map_err(|e| RemoteError::Refused(e.to_string()))
    }) {
        Ok(session) => {
            let tokens = tokens_of(&session);
            let _ = state.quietly(|s| s.store.sync_set_tokens(Some(&tokens)));
            Ok(tokens)
        }
        Err(RemoteError::SignedOut | RemoteError::Refused(_)) => {
            let _ = state.quietly(|s| s.store.sync_set_tokens(None));
            Err(RemoteError::SignedOut)
        }
        Err(other) => Err(other),
    }
}

/// The tokens stored now. They are refreshed when the server refuses them,
/// never by comparing their expiry with this device's clock: a device whose
/// clock is wrong would refresh on every call, or never.
fn current_tokens(state: &AppState) -> Result<Tokens, RemoteError> {
    state
        .quietly(|s| s.store.sync_link())
        .map_err(|_| RemoteError::Offline("the vault is locked".into()))?
        .and_then(|link| link.tokens)
        .ok_or(RemoteError::SignedOut)
}

impl Wire<'_> {
    fn refresh(&mut self) -> Result<(), RemoteError> {
        self.tokens = refresh(self.state, &self.api, &self.tokens.access)?;
        Ok(())
    }

    /// As [`Self::call`], keeping the server's answer for the file calls,
    /// whose refusals carry a code to act on.
    fn file_call(
        &mut self,
        send: impl Fn(&Api, &str) -> Result<http::Answer, HttpError>,
    ) -> Result<http::Answer, HttpError> {
        match send(&self.api, &self.tokens.access) {
            Err(HttpError::Api { status: 401, .. }) => {
                if let Err(e) = self.refresh() {
                    return Err(HttpError::Offline(e.to_string()));
                }
                send(&self.api, &self.tokens.access)
            }
            other => other,
        }
    }

    fn call(
        &mut self,
        send: impl Fn(&Api, &str) -> Result<http::Answer, HttpError>,
    ) -> Result<http::Answer, RemoteError> {
        match send(&self.api, &self.tokens.access).map_err(remote_error) {
            Err(RemoteError::SignedOut) => {
                self.refresh()?;
                send(&self.api, &self.tokens.access).map_err(remote_error)
            }
            other => other,
        }
    }
}

/// `space=` for a shared space's calls, nothing for the personal space.
fn in_space(space: Option<&str>, sep: char) -> String {
    space.map(|s| format!("{sep}space={s}")).unwrap_or_default()
}

impl Remote for Wire<'_> {
    fn heavy(
        &mut self,
        space: Option<&str>,
        min: i64,
    ) -> Result<app_protocol::sync::HeavyDocs, RemoteError> {
        let path = format!("/sync/heavy?min={min}{}", in_space(space, '&'));
        match self.call(|api, token| api.get(&path, Some(token))) {
            Ok(answer) => answer
                .parse()
                .map_err(|e| RemoteError::Refused(e.to_string())),
            // a server from before compaction: nothing to compact
            Err(RemoteError::Refused(_)) => Ok(app_protocol::sync::HeavyDocs { docs: Vec::new() }),
            Err(other) => Err(other),
        }
    }

    fn push(
        &mut self,
        space: Option<&str>,
        request: &PushRequest,
    ) -> Result<PushAnswer, RemoteError> {
        let path = format!("/sync/push{}", in_space(space, '?'));
        self.call(|api, token| api.post(&path, Some(token), request))?
            .parse()
            .map_err(|e| RemoteError::Refused(e.to_string()))
    }

    fn pull(
        &mut self,
        space: Option<&str>,
        after: i64,
        limit: usize,
    ) -> Result<PullAnswer, RemoteError> {
        let path = format!(
            "/sync/changes?after={after}&limit={limit}{}",
            in_space(space, '&')
        );
        self.call(|api, token| api.get(&path, Some(token)))?
            .parse()
            .map_err(|e| RemoteError::Refused(e.to_string()))
    }

    fn spaces(&mut self) -> Result<Option<app_protocol::sync::SharedSpaces>, RemoteError> {
        match self.call(|api, token| api.get("/sync/spaces", Some(token))) {
            Ok(answer) => answer
                .parse()
                .map(Some)
                .map_err(|e| RemoteError::Refused(e.to_string())),
            // a server from before sharing: the shared spaces stay as they are
            Err(RemoteError::Refused(_)) => Ok(None),
            Err(other) => Err(other),
        }
    }

    fn give_grants(
        &mut self,
        space: &str,
        grants: &app_protocol::sync::GiveGrants,
    ) -> Result<(), RemoteError> {
        let path = format!("/sync/spaces/{space}/grants");
        self.call(|api, token| api.post(&path, Some(token), grants))
            .map(drop)
    }

    fn files_info(&mut self) -> Result<Option<app_protocol::files::FilesInfo>, RemoteError> {
        let info = match self.call(|api, token| api.get("/sync/files/policy", Some(token))) {
            Ok(answer) => Some(
                answer
                    .parse::<app_protocol::files::FilesInfo>()
                    .map_err(|e| RemoteError::Refused(e.to_string()))?,
            ),
            // a server from before files: they stay on the devices
            Err(RemoteError::Refused(_)) => None,
            Err(other) => return Err(other),
        };
        self.state.sync.lock().files.clone_from(&info);
        Ok(info)
    }

    fn file_state(
        &mut self,
        space: Option<&str>,
        address: &str,
    ) -> Result<app_protocol::files::FileState, FileError> {
        let path = format!("/sync/files/{address}/state{}", in_space(space, '?'));
        files::answer(self.file_call(|api, token| api.get(&path, Some(token))))
    }

    fn put_file(
        &mut self,
        space: Option<&str>,
        address: &str,
        part: FilePart<'_>,
    ) -> Result<app_protocol::files::FileState, FileError> {
        let path = format!("/sync/files/{address}{}", in_space(space, '?'));
        let headers = files::part_headers(&part);
        files::answer(
            self.file_call(|api, token| api.put_bytes(&path, token, &headers, part.bytes)),
        )
    }

    fn get_file(
        &mut self,
        space: Option<&str>,
        address: &str,
    ) -> Result<Box<dyn std::io::Read + Send>, FileError> {
        let path = format!("/sync/files/{address}{}", in_space(space, '?'));
        match self.api.get_stream(&path, &self.tokens.access) {
            Err(HttpError::Api { status: 401, .. }) => {
                self.refresh()?;
                self.api
                    .get_stream(&path, &self.tokens.access)
                    .map_err(files::file_error)
            }
            other => other.map_err(files::file_error),
        }
    }

    fn keep_file(&mut self, space: Option<&str>, address: &str) -> Result<(), FileError> {
        let path = format!("/sync/files/{address}/keep{}", in_space(space, '?'));
        self.file_call(|api, token| api.post(&path, Some(token), &()))
            .map(drop)
            .map_err(files::file_error)
    }

    fn release_file(
        &mut self,
        space: Option<&str>,
        address: &str,
        head: i64,
    ) -> Result<bool, FileError> {
        let path = format!("/sync/files/{address}?head={head}{}", in_space(space, '&'));
        match self.file_call(|api, token| api.delete(&path, Some(token))) {
            Ok(_) => Ok(true),
            Err(HttpError::Api { status: 409, .. }) => Ok(false),
            Err(other) => Err(files::file_error(other)),
        }
    }
}
