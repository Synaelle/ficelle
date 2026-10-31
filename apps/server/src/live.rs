// SPDX-License-Identifier: AGPL-3.0-or-later

//! Who is listening for changes: one broadcast channel per account with
//! devices connected, made on first use and dropped with its last listener.
//! A push tells the channels of everyone in the space, the live sockets
//! pass it on, devices pull. Keyed by account rather than space, a socket
//! hears of every space its account is in, shared ones joined later
//! included. Notices carry no content, only that there is something to
//! pull.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use app_protocol::sync::Viewer;
use tokio::sync::broadcast;

/// Notices kept for a slow socket before it skips ahead. A skipped notice
/// costs nothing: the next one, or the lag itself, makes the device pull.
const BACKLOG: usize = 64;

/// Something happened that connected devices should hear about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    /// A device pushed changes to a space, or the account's shared spaces
    /// changed (head 0, no device).
    Changed {
        /// Number of the latest change now.
        head: i64,
        /// The device that pushed, it already has them.
        device: String,
        /// The shared space, none for the account's own.
        space: Option<String>,
    },
    /// The admin turned co-editing on or off.
    Settings {
        /// The new setting.
        coediting: bool,
    },
    /// Who views the notes of a shared space now.
    Presence {
        /// The space.
        space: String,
        /// Everyone viewing one of its notes.
        viewers: Vec<Viewer>,
    },
}

/// A live socket viewing a note of a shared space.
#[derive(Debug, Clone)]
struct Seat {
    socket: u64,
    viewer: Viewer,
}

/// The channels, shared by every request.
#[derive(Debug, Default)]
pub struct Live {
    spaces: Mutex<HashMap<String, broadcast::Sender<Notice>>>,
    /// Who views what, by shared space. Kept in memory only: a restart
    /// forgets it and the devices say it again when they reconnect.
    seats: Mutex<HashMap<String, Vec<Seat>>>,
    sockets: AtomicU64,
}

impl Live {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, broadcast::Sender<Notice>>> {
        self.spaces
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Starts listening for an account.
    pub fn subscribe(&self, account: &str) -> broadcast::Receiver<Notice> {
        self.lock()
            .entry(account.to_owned())
            .or_insert_with(|| broadcast::channel(BACKLOG).0)
            .subscribe()
    }

    /// Tells the account's listeners, if any.
    pub fn changed(&self, account: &str, head: i64, device: &str, space: Option<&str>) {
        let mut accounts = self.lock();
        if let Some(sender) = accounts.get(account) {
            if sender.receiver_count() == 0 {
                accounts.remove(account);
            } else {
                let _ = sender.send(Notice::Changed {
                    head,
                    device: device.to_owned(),
                    space: space.map(str::to_owned),
                });
            }
        }
    }

    /// Tells these accounts their shared spaces changed: someone joined or
    /// left, a key came.
    pub fn spaces_changed(&self, accounts: &[String]) {
        for account in accounts {
            self.changed(account, 0, "", None);
        }
    }

    /// A number for a new live socket, to know its seats.
    pub fn socket_id(&self) -> u64 {
        self.sockets.fetch_add(1, Ordering::Relaxed)
    }

    fn seats(&self) -> std::sync::MutexGuard<'_, HashMap<String, Vec<Seat>>> {
        self.seats
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// A socket now views this note of a shared space, or nothing (`None`).
    /// Returns the spaces whose viewers changed.
    pub fn view(&self, socket: u64, seen: Option<(String, Viewer)>) -> Vec<String> {
        let mut seats = self.seats();
        let mut changed = Vec::new();
        for (space, list) in seats.iter_mut() {
            let before = list.len();
            list.retain(|s| s.socket != socket);
            if list.len() != before {
                changed.push(space.clone());
            }
        }
        if let Some((space, viewer)) = seen {
            let list = seats.entry(space.clone()).or_default();
            list.push(Seat { socket, viewer });
            if !changed.contains(&space) {
                changed.push(space);
            }
        }
        seats.retain(|_, list| !list.is_empty());
        changed
    }

    /// Forgets who views what. Returns the spaces that had viewers.
    pub fn clear_seats(&self) -> Vec<String> {
        self.seats().drain().map(|(space, _)| space).collect()
    }

    /// Who views the notes of a space, each person and note once.
    pub fn viewers(&self, space: &str) -> Vec<Viewer> {
        let mut out: Vec<Viewer> = self
            .seats()
            .get(space)
            .map(|list| list.iter().map(|s| s.viewer.clone()).collect())
            .unwrap_or_default();
        out.sort_by(|a, b| (&a.name, &a.note).cmp(&(&b.name, &b.note)));
        out.dedup();
        out
    }

    /// Tells an account's listeners who views a space now.
    pub fn presence(&self, account: &str, space: &str, viewers: &[Viewer]) {
        if let Some(sender) = self.lock().get(account) {
            let _ = sender.send(Notice::Presence {
                space: space.to_owned(),
                viewers: viewers.to_vec(),
            });
        }
    }

    /// Tells every listener of a new co-editing setting.
    pub fn settings(&self, coediting: bool) {
        let mut spaces = self.lock();
        spaces.retain(|_, sender| sender.receiver_count() > 0);
        for sender in spaces.values() {
            let _ = sender.send(Notice::Settings { coediting });
        }
    }

    /// Accounts with a listener.
    #[cfg(test)]
    pub fn listening(&self) -> usize {
        self.lock()
            .values()
            .filter(|s| s.receiver_count() > 0)
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notices_reach_the_listeners_of_their_account_only() {
        let live = Live::default();
        let mut a = live.subscribe("a");
        let mut b = live.subscribe("b");
        live.changed("a", 3, "phone", Some("s"));
        assert_eq!(
            a.try_recv().unwrap(),
            Notice::Changed {
                head: 3,
                device: "phone".into(),
                space: Some("s".into())
            }
        );
        assert!(b.try_recv().is_err());
        live.settings(false);
        assert_eq!(a.try_recv().unwrap(), Notice::Settings { coediting: false });
        assert_eq!(b.try_recv().unwrap(), Notice::Settings { coediting: false });
        drop(b);
        live.changed("b", 1, "x", None);
        assert_eq!(live.listening(), 1, "a channel without listeners goes");
        live.changed("nobody", 1, "x", None);
    }

    #[test]
    fn seats_follow_what_each_socket_views() {
        let live = Live::default();
        let (one, two) = (live.socket_id(), live.socket_id());
        let viewer = |name: &str, note: &str| Viewer {
            name: name.into(),
            display_name: name.into(),
            note: note.into(),
        };
        assert_eq!(
            live.view(one, Some(("s".into(), viewer("sam", "n1")))),
            ["s"]
        );
        assert_eq!(
            live.view(two, Some(("s".into(), viewer("sam", "n1")))),
            ["s"]
        );
        assert_eq!(
            live.viewers("s"),
            [viewer("sam", "n1")],
            "two devices, one viewer"
        );
        assert_eq!(
            live.view(one, Some(("t".into(), viewer("sam", "n2")))),
            ["s", "t"]
        );
        assert_eq!(live.view(two, None), ["s"]);
        assert!(live.viewers("s").is_empty());
        assert_eq!(live.view(one, None), ["t"]);
        assert!(live.seats().is_empty(), "nothing kept for nobody");
    }
}
