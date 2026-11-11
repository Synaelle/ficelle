// SPDX-License-Identifier: AGPL-3.0-or-later

//! The node: listens, advertises and browses with mDNS, dials the paired
//! devices it finds, runs pairing on both sides, and keeps one session per
//! device.

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpListener, TcpStream, UdpSocket};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use serde::Serialize;

use crate::channel::{self, Channel, WANTS_PAIRING, WANTS_SYNC};
use crate::session::{self, SETTLE};
use crate::{Error, Host, Me, store};

/// How long a code stays good.
const CODE_FOR: Duration = Duration::from_mins(5);
/// Wrong codes before the code is gone.
const TRIES: u8 = 3;
/// How long a device typing a code looks for the one showing it.
const LOOK_FOR: Duration = Duration::from_secs(8);
const CONNECT: Duration = Duration::from_secs(3);
/// After a failed dial, the same address waits this long.
const RETRY_AFTER: Duration = Duration::from_secs(20);
/// After a device that is not paired here, or on another version.
const RETRY_STRANGER: Duration = Duration::from_mins(5);
const TICK: Duration = Duration::from_millis(500);
/// How often the vault is looked at for something written here.
const WRITES_EVERY: Duration = Duration::from_millis(250);
/// A device dials have failed to reach, the way a firewall makes them fail,
/// for this long: the settings say what to allow.
pub(crate) const BLOCKED_AFTER: Duration = Duration::from_mins(1);

/// Where a sync with one device stands. A device with none is not
/// connected now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum Live {
    /// Changes are going across.
    Syncing,
    /// Connected and caught up.
    Synced,
    /// It runs an older version: it has to update.
    NeedsUpdate {
        /// Its version.
        version: String,
    },
    /// It runs a newer version: this device has to update.
    UpdateThis {
        /// Its version.
        version: String,
    },
    /// The last connection failed.
    Failed {
        /// Why, in English.
        error: String,
    },
}

/// A code on show, for the pairing screen.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingStatus {
    /// Six digits, empty once failed or paired.
    pub code: String,
    /// Until the code is gone.
    pub seconds_left: u64,
    /// `ip:port` to type on a network where devices cannot see each other.
    pub address: Option<String>,
    /// `tooManyTries` or `expired`.
    pub failed: Option<&'static str>,
    /// The name of the device that typed the code.
    pub paired: Option<String>,
}

/// What the node is doing, for the settings and the sidebar.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// Listening and advertising.
    pub running: bool,
    /// The port listened on.
    pub port: Option<u16>,
    /// Why devices may not find this one: mDNS could not start.
    pub discovery_error: Option<String>,
    /// The code on show here, or how showing it ended.
    pub pairing: Option<PairingStatus>,
    /// Per device key (hex), how the sync with it stands.
    pub live: HashMap<String, Live>,
    /// Device keys (hex) seen but out of reach for a while, as when a
    /// firewall on one of the two drops the connection.
    pub blocked: Vec<String>,
}

struct Identity {
    secret: [u8; 32],
    public: [u8; 32],
}

struct Pairing {
    code: String,
    until: Instant,
    tries: u8,
    /// A pairing handshake is under way: others wait their turn.
    busy: bool,
    /// Codes checked, for the tests.
    #[cfg(test)]
    checked: u8,
    failed: Option<&'static str>,
    paired: Option<String>,
}

struct Slot {
    id: u64,
    /// A write here since the last round, at that time.
    kicked: Option<Instant>,
    /// A round now, without waiting for quiet.
    now: bool,
    stop: bool,
    /// The address dialled, when this side dialled.
    address: Option<SocketAddr>,
    /// Files opened here that the other device may have, by hash.
    wants: Vec<String>,
}

struct Running {
    generation: u64,
    port: u16,
    mdns: Option<ServiceDaemon>,
    instance: String,
}

/// mDNS answers, by full name.
struct Found {
    addresses: Vec<SocketAddr>,
    pairing: bool,
}

#[derive(Default)]
struct State {
    wanted: bool,
    /// A device typing a code: discovery runs for it.
    typing: bool,
    running: Option<Running>,
    generation: u64,
    next_id: u64,
    identity: Option<Identity>,
    sessions: HashMap<[u8; 32], Slot>,
    live: HashMap<[u8; 32], Live>,
    pairing: Option<Pairing>,
    found: HashMap<String, Found>,
    /// A device not to try at an address before then.
    cooldown: HashMap<(SocketAddr, [u8; 32]), Instant>,
    dialing: HashSet<(SocketAddr, [u8; 32])>,
    discovery_error: Option<String>,
    /// Since when dials to a device fail as a firewall makes them fail,
    /// until a connection with it works, either way.
    unreachable: HashMap<[u8; 32], Instant>,
}

pub(crate) struct Shared<H: Host> {
    pub host: Arc<H>,
    pub me: Me,
    state: Mutex<State>,
    wake: Condvar,
    /// The port asked for: release and dev builds each have their own.
    preferred_port: u16,
    service: String,
}

/// The local sync of this device. Cheap to clone.
pub struct Node<H: Host> {
    pub(crate) shared: Arc<Shared<H>>,
}

impl<H: Host> Clone for Node<H> {
    fn clone(&self) -> Self {
        Self {
            shared: self.shared.clone(),
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

fn random_hex(len: usize) -> String {
    let mut bytes = vec![0u8; len / 2];
    // a fixed name only costs privacy, never correctness
    let _ = getrandom::fill(&mut bytes);
    hex(&bytes)
}

fn six_digits() -> Result<String, Error> {
    let mut bytes = [0u8; 4];
    getrandom::fill(&mut bytes).map_err(|e| Error::Io(std::io::Error::other(e.to_string())))?;
    Ok(format!("{:06}", u32::from_le_bytes(bytes) % 1_000_000))
}

/// This device's address on the network the default route leaves by. A
/// UDP socket "connected" sends nothing.
fn own_ip() -> Option<Ipv4Addr> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect((Ipv4Addr::new(192, 0, 2, 1), 9)).ok()?;
    match socket.local_addr().ok()?.ip() {
        std::net::IpAddr::V4(ip) if !ip.is_unspecified() => Some(ip),
        _ => None,
    }
}

/// `ip:port` or an ip alone, with this build's port.
fn parse_address(text: &str, port: u16) -> Option<SocketAddr> {
    let text = text.trim();
    text.parse::<SocketAddr>().ok().or_else(|| {
        text.parse::<Ipv4Addr>()
            .ok()
            .map(|ip| SocketAddr::V4(SocketAddrV4::new(ip, port)))
    })
}

impl<H: Host> Shared<H> {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// This device's public key, zeros before there is one.
    pub(crate) fn public(&self) -> [u8; 32] {
        self.lock().identity.as_ref().map_or([0; 32], |i| i.public)
    }

    fn generation_alive(&self, generation: u64) -> bool {
        self.lock()
            .running
            .as_ref()
            .is_some_and(|r| r.generation == generation)
    }

    pub(crate) fn set_live(&self, key: &[u8; 32], live: Live) {
        let changed = self.lock().live.insert(*key, live.clone()) != Some(live);
        if changed {
            self.host.status_changed();
        }
    }

    pub(crate) fn session_wanted(&self, key: &[u8; 32], id: u64) -> bool {
        let state = self.lock();
        state.running.is_some()
            && state
                .sessions
                .get(key)
                .is_some_and(|slot| slot.id == id && !slot.stop)
    }

    pub(crate) fn take_kick(&self, key: &[u8; 32], id: u64) -> bool {
        let mut state = self.lock();
        let Some(slot) = state.sessions.get_mut(key).filter(|s| s.id == id) else {
            return false;
        };
        let due = slot.now || slot.kicked.is_some_and(|at| at.elapsed() >= SETTLE);
        if due {
            slot.now = false;
            slot.kicked = None;
        }
        due
    }

    /// Something was written here: each session starts a round once the
    /// writing pauses.
    fn kick(&self) {
        let mut state = self.lock();
        let now = Instant::now();
        for slot in state.sessions.values_mut() {
            slot.kicked.get_or_insert(now);
        }
    }

    pub(crate) fn take_wants(&self, key: &[u8; 32], id: u64) -> Vec<String> {
        let mut state = self.lock();
        state
            .sessions
            .get_mut(key)
            .filter(|s| s.id == id)
            .map(|s| std::mem::take(&mut s.wants))
            .unwrap_or_default()
    }

    pub(crate) fn ended(&self, key: &[u8; 32], id: u64, error: Option<Error>) {
        let mut state = self.lock();
        if state.sessions.get(key).is_some_and(|s| s.id == id) {
            state.sessions.remove(key);
            match error {
                None | Some(Error::Closed | Error::Timeout | Error::Locked) => {
                    if matches!(state.live.get(key), Some(Live::Syncing | Live::Synced)) {
                        state.live.remove(key);
                    }
                }
                Some(err) => {
                    state.live.insert(
                        *key,
                        Live::Failed {
                            error: err.to_string(),
                        },
                    );
                }
            }
        }
        drop(state);
        self.wake.notify_all();
        self.host.status_changed();
    }

    /// Takes a session with `key`, unless one is already up that wins: of
    /// two connections between the same devices, the one dialled by the
    /// lower key stays, so both sides keep the same one.
    fn admit(&self, key: [u8; 32], dialled: Option<SocketAddr>) -> Option<u64> {
        let public = self.public();
        let mut state = self.lock();
        let new_wins = if dialled.is_some() {
            public < key
        } else {
            key < public
        };
        if let Some(old) = state.sessions.get_mut(&key) {
            if !new_wins {
                return None;
            }
            old.stop = true;
        }
        state.unreachable.remove(&key);
        state.next_id += 1;
        let id = state.next_id;
        state.sessions.insert(
            key,
            Slot {
                id,
                kicked: None,
                now: false,
                stop: false,
                address: dialled,
                wants: Vec::new(),
            },
        );
        Some(id)
    }

    /// Moves the first failure toward a device back in time, as if dials
    /// had failed for that long.
    #[cfg(test)]
    pub(crate) fn unreachable_for(&self, key: [u8; 32], long: Duration) -> bool {
        let mut state = self.lock();
        let Some(since) = state.unreachable.get_mut(&key) else {
            return false;
        };
        *since -= long;
        true
    }

    /// Discovery found a Ficelle at `address`, for tests.
    #[cfg(test)]
    pub(crate) fn announce(&self, address: SocketAddr) {
        self.lock().found.insert(
            address.to_string(),
            Found {
                addresses: vec![address],
                pairing: false,
            },
        );
    }

    /// The port this device listens on, while it runs.
    pub(crate) fn port(&self) -> Option<u16> {
        self.lock().running.as_ref().map(|r| r.port)
    }

    /// Notes a dial that failed the way a firewall makes it fail: no answer
    /// at all, or a refusal from an address discovery just announced (the
    /// app is open there, yet its port refuses). A refusal at a remembered
    /// address only means the app is closed. A phone at a remembered address
    /// is left out: in a pocket it stops answering, by design.
    pub(crate) fn failed_like_a_firewall(
        &self,
        address: SocketAddr,
        key: [u8; 32],
        err: &std::io::Error,
    ) {
        let announced = self
            .lock()
            .found
            .values()
            .any(|f| f.addresses.contains(&address));
        let silent = matches!(
            err.kind(),
            std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
        );
        let refused = err.kind() == std::io::ErrorKind::ConnectionRefused;
        let phone = || {
            store(&*self.host, |s, _| s.peer_devices()).is_ok_and(|devices| {
                devices
                    .iter()
                    .any(|d| d.key == key && d.platform == "android")
            })
        };
        if (announced && (silent || refused)) || (silent && !phone()) {
            self.lock()
                .unreachable
                .entry(key)
                .or_insert_with(Instant::now);
        }
    }

    fn secret(&self) -> Option<[u8; 32]> {
        self.lock().identity.as_ref().map(|i| i.secret)
    }

    /// Whether `key` is a device paired here, not forgotten.
    fn paired(&self, key: &[u8; 32]) -> bool {
        store(&*self.host, |s, _| s.peer_devices())
            .is_ok_and(|devices| devices.iter().any(|d| d.key == key && d.active()))
    }

    fn cool(&self, address: SocketAddr, key: [u8; 32], wait: Duration) {
        self.lock()
            .cooldown
            .insert((address, key), Instant::now() + wait);
    }

    /// After the handshake of a sync connection, either way.
    fn sync_session(self: &Arc<Self>, ch: Channel, dialled: Option<SocketAddr>) {
        let key = ch.remote_key;
        if !self.paired(&key) {
            ch.close();
            if let Some(address) = dialled {
                self.cool(address, key, RETRY_STRANGER);
            }
            return;
        }
        self.start_session(ch, dialled, dialled.is_some());
    }

    /// Hellos, version check, then the session on its own thread.
    fn start_session(self: &Arc<Self>, mut ch: Channel, dialled: Option<SocketAddr>, starts: bool) {
        let key = ch.remote_key;
        let Ok(hello) = session::hellos(self, &mut ch) else {
            ch.close();
            return;
        };
        if let Err(live) = session::check_hello(self, &mut ch, &hello, dialled) {
            ch.close();
            if let Some(address) = dialled {
                self.cool(address, key, RETRY_STRANGER);
            }
            self.set_live(&key, live);
            return;
        }
        let Some(id) = self.admit(key, dialled) else {
            ch.close();
            return;
        };
        self.host.status_changed();
        let shared = self.clone();
        let spawned = std::thread::Builder::new()
            .name("peer-session".into())
            .spawn(move || session::run(&shared, ch, id, starts));
        if spawned.is_err() {
            self.lock().sessions.remove(&key);
        }
    }

    fn accept(self: &Arc<Self>, mut stream: TcpStream) {
        let _ = stream.set_read_timeout(Some(CONNECT));
        let mut first = [0u8; 1];
        if stream.read_exact(&mut first).is_err() {
            return;
        }
        let Some(secret) = self.secret() else { return };
        match first[0] {
            WANTS_SYNC => {
                if let Ok(ch) = channel::answer_sync(stream, &secret, |k| self.paired(k)) {
                    self.sync_session(ch, None);
                }
            }
            WANTS_PAIRING => self.answer_pairing(stream, &secret),
            _ => {}
        }
    }

    #[cfg(test)]
    pub(crate) fn codes_checked(&self) -> u8 {
        self.lock().pairing.as_ref().map_or(0, |p| p.checked)
    }

    fn answer_pairing(self: &Arc<Self>, stream: TcpStream, secret: &[u8; 32]) {
        // one try at a time, counted before the code is checked: tries
        // arriving together cannot all read the code before one fails
        let code = {
            let mut state = self.lock();
            match state.pairing.as_mut() {
                Some(p)
                    if !p.busy
                        && p.tries > 0
                        && p.failed.is_none()
                        && p.paired.is_none()
                        && Instant::now() < p.until =>
                {
                    p.busy = true;
                    p.tries -= 1;
                    #[cfg(test)]
                    {
                        p.checked += 1;
                    }
                    p.code.clone()
                }
                _ => return,
            }
        };
        let answered = channel::answer_pairing(stream, secret, &code);
        if let Some(p) = self.lock().pairing.as_mut() {
            p.busy = false;
            if answered.is_err() && p.tries == 0 {
                p.failed = Some("tooManyTries");
            }
        }
        let Ok(mut ch) = answered else {
            self.host.status_changed();
            return;
        };

        if let Ok(name) = self.pair(&mut ch) {
            if let Some(p) = self.lock().pairing.as_mut() {
                p.paired = Some(name);
            }
            self.host.status_changed();
            self.start_session(ch, None, false);
        } else {
            ch.close();
        }
    }

    /// After a pairing handshake, either side: hellos, then pins the other
    /// device. Its name.
    fn pair(&self, ch: &mut Channel) -> Result<String, Error> {
        let hello = session::hellos(self, ch).map_err(|e| match e {
            // the side showing the code hangs up on a wrong one
            Error::Closed | Error::Timeout | Error::Io(_) => Error::WrongCode,
            other => other,
        })?;
        let key = ch.remote_key;
        let pinned = store(&*self.host, |s, _| {
            s.peer_pair(&key, &hello.name, &hello.platform, &hello.version)
        });
        if let Err(e) = pinned {
            let _ = ch.send(&crate::wire::encode(&crate::wire::Msg::Bye {
                reason: "server".into(),
            }));
            return Err(e);
        }
        Ok(hello.name)
    }

    /// Tries one paired device at one address.
    fn dial(self: &Arc<Self>, address: SocketAddr, key: [u8; 32]) {
        let outcome = (|| {
            let secret = self.secret().ok_or(Error::Locked)?;
            let stream = TcpStream::connect_timeout(&address, CONNECT)?;
            channel::dial_sync(stream, &secret, &key)
        })();
        self.lock().dialing.remove(&(address, key));
        match outcome {
            Ok(ch) => self.sync_session(ch, Some(address)),
            // another device, or not one that knows this one
            Err(Error::Handshake(_) | Error::Closed | Error::Timeout) => {
                self.cool(address, key, RETRY_STRANGER);
            }
            Err(Error::Io(err)) => {
                self.failed_like_a_firewall(address, key, &err);
                self.cool(address, key, RETRY_AFTER);
            }
            Err(_) => self.cool(address, key, RETRY_AFTER),
        }
    }

    /// Paired devices with no session, and where to try each: the address
    /// it last answered on, and every Ficelle discovery found, since the
    /// advertisements do not say whose they are.
    fn targets(&self) -> Vec<(SocketAddr, [u8; 32])> {
        let Ok(devices) = store(&*self.host, |s, _| s.peer_devices()) else {
            return Vec::new();
        };
        let state = self.lock();
        let missing: Vec<([u8; 32], Option<SocketAddr>)> = devices
            .iter()
            .filter(|d| d.active())
            .filter_map(|d| {
                let key: [u8; 32] = d.key.as_slice().try_into().ok()?;
                let wanted = !state.sessions.contains_key(&key)
                    && !matches!(
                        state.live.get(&key),
                        Some(Live::NeedsUpdate { .. } | Live::UpdateThis { .. })
                    );
                wanted.then(|| (key, d.address.as_deref().and_then(|a| a.parse().ok())))
            })
            .collect();
        if missing.is_empty() {
            return Vec::new();
        }
        let connected: HashSet<SocketAddr> =
            state.sessions.values().filter_map(|s| s.address).collect();
        let now = Instant::now();
        let own = state.running.as_ref().map(|r| r.instance.clone());
        let mut targets: Vec<(SocketAddr, [u8; 32])> = missing
            .iter()
            .filter_map(|(key, address)| address.map(|a| (a, *key)))
            .collect();
        for (name, found) in &state.found {
            if own.as_deref().is_some_and(|own| name.starts_with(own)) {
                continue;
            }
            for address in &found.addresses {
                targets.extend(missing.iter().map(|(key, _)| (*address, *key)));
            }
        }
        targets.sort();
        targets.dedup();
        targets.retain(|target| {
            !connected.contains(&target.0)
                && !state.dialing.contains(target)
                && state.cooldown.get(target).is_none_or(|until| now >= *until)
        });
        targets
    }

    fn dialer(self: &Arc<Self>, generation: u64) {
        while self.generation_alive(generation) {
            for (address, key) in self.targets() {
                self.lock().dialing.insert((address, key));
                let shared = self.clone();
                let spawned = std::thread::Builder::new()
                    .name("peer-dial".into())
                    .spawn(move || shared.dial(address, key));
                if spawned.is_err() {
                    self.lock().dialing.remove(&(address, key));
                }
            }
            let state = self.lock();
            let _ = self.wake.wait_timeout(state, Duration::from_secs(2));
        }
    }

    /// Follows the writes made here, by the mark the vault keeps of them,
    /// and starts a round for each. The first look starts one too, so a
    /// write made just before it is never missed.
    fn follow_writes(self: &Arc<Self>, generation: u64) {
        let mut seen: Option<Option<i64>> = None;
        while self.generation_alive(generation) {
            if let Ok(mark) = store(&*self.host, |s, _| s.peer_write_mark())
                && seen != Some(mark)
            {
                self.kick();
                seen = Some(mark);
            }
            std::thread::sleep(WRITES_EVERY);
        }
    }

    fn listen(self: &Arc<Self>, listener: &TcpListener, generation: u64) {
        while self.generation_alive(generation) {
            match listener.accept() {
                Ok((stream, _)) => {
                    let _ = stream.set_nonblocking(false);
                    let shared = self.clone();
                    let _ = std::thread::Builder::new()
                        .name("peer-accept".into())
                        .spawn(move || shared.accept(stream));
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(_) => std::thread::sleep(TICK),
            }
        }
    }

    fn browse(self: &Arc<Self>, receiver: &mdns_sd::Receiver<ServiceEvent>, generation: u64) {
        while self.generation_alive(generation) {
            let Ok(event) = receiver.recv_timeout(TICK) else {
                continue;
            };
            match event {
                ServiceEvent::ServiceResolved(info) => {
                    let addresses: Vec<SocketAddr> = info
                        .addresses
                        .iter()
                        .map(mdns_sd::ScopedIp::to_ip_addr)
                        .filter(std::net::IpAddr::is_ipv4)
                        .map(|ip| SocketAddr::new(ip, info.port))
                        .collect();
                    let pairing = info.txt_properties.get_property_val_str("p") == Some("1");
                    self.lock()
                        .found
                        .insert(info.fullname.clone(), Found { addresses, pairing });
                    self.wake.notify_all();
                }
                ServiceEvent::ServiceRemoved(_, fullname) => {
                    self.lock().found.remove(&fullname);
                }
                _ => {}
            }
        }
    }

    fn advertise(&self, running: &Running, pairing: bool) -> Result<(), String> {
        let Some(mdns) = &running.mdns else {
            return Ok(());
        };
        let mut properties = vec![("v", app_protocol::local::PROTOCOL.to_string())];
        if pairing {
            properties.push(("p", "1".to_owned()));
        }
        let host = format!("{}.local.", running.instance);
        let info = ServiceInfo::new(
            &self.service,
            &running.instance,
            &host,
            "",
            running.port,
            &properties[..],
        )
        .map_err(|e| e.to_string())?
        .enable_addr_auto();
        mdns.register(info).map_err(|e| e.to_string())
    }

    /// Starts or stops listening and discovery as the state asks.
    fn refresh(self: &Arc<Self>) {
        let pairing_open = {
            let state = self.lock();
            state.typing
                || state
                    .pairing
                    .as_ref()
                    .is_some_and(|p| p.paired.is_none() && p.failed.is_none())
        };
        let paired = store(&*self.host, |s, _| s.devices_paired()).unwrap_or(false);
        let want = self.lock().wanted && (paired || pairing_open);
        let up = self.lock().running.is_some();
        if want && !up {
            self.start(pairing_open);
        } else if !want && up {
            self.stop();
        } else if want {
            // the pairing mark in the advertisement follows the code
            let state = self.lock();
            if let Some(running) = &state.running {
                let shows = state
                    .pairing
                    .as_ref()
                    .is_some_and(|p| p.paired.is_none() && p.failed.is_none());
                let _ = self.advertise(running, shows);
            }
        }
    }

    fn load_identity(&self) -> Result<(), Error> {
        let secret = store(&*self.host, |s, _| s.peer_secret())?;
        let secret = if let Some(secret) = secret {
            *secret
        } else {
            let secret = channel::new_secret()?;
            store(&*self.host, |s, _| s.set_peer_secret(&secret))?;
            secret
        };
        let public = channel::public_key(&secret)?;
        self.lock().identity = Some(Identity { secret, public });
        Ok(())
    }

    fn start(self: &Arc<Self>, pairing: bool) {
        if self.load_identity().is_err() {
            return;
        }
        let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, self.preferred_port))
            .or_else(|_| TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0)));
        let Ok(listener) = listener else { return };
        let Ok(port) = listener.local_addr().map(|a| a.port()) else {
            return;
        };
        if listener.set_nonblocking(true).is_err() {
            return;
        }
        let (mdns, discovery_error) = if self.service.is_empty() {
            (None, None)
        } else {
            match ServiceDaemon::new() {
                Ok(mdns) => (Some(mdns), None),
                Err(e) => (None, Some(e.to_string())),
            }
        };
        let receiver = mdns.as_ref().and_then(|m| m.browse(&self.service).ok());
        let generation = {
            let mut state = self.lock();
            state.generation += 1;
            state.generation
        };
        let running = Running {
            generation,
            port,
            mdns,
            instance: random_hex(16),
        };
        let advertised = self.advertise(&running, pairing).err();
        {
            let mut state = self.lock();
            state.discovery_error = discovery_error.or(advertised);
            state.running = Some(running);
            state.found.clear();
            state.cooldown.clear();
            state.unreachable.clear();
        }
        let shared = self.clone();
        let _ = std::thread::Builder::new()
            .name("peer-listen".into())
            .spawn(move || shared.listen(&listener, generation));
        let shared = self.clone();
        let _ = std::thread::Builder::new()
            .name("peer-dialer".into())
            .spawn(move || shared.dialer(generation));
        let shared = self.clone();
        let _ = std::thread::Builder::new()
            .name("peer-writes".into())
            .spawn(move || shared.follow_writes(generation));
        if let Some(receiver) = receiver {
            let shared = self.clone();
            let _ = std::thread::Builder::new()
                .name("peer-browse".into())
                .spawn(move || shared.browse(&receiver, generation));
        }
        self.host.status_changed();
    }

    fn stop(&self) {
        let mut state = self.lock();
        if let Some(running) = state.running.take()
            && let Some(mdns) = running.mdns
        {
            let _ = mdns.shutdown();
        }
        for slot in state.sessions.values_mut() {
            slot.stop = true;
        }
        state.found.clear();
        drop(state);
        self.wake.notify_all();
        self.host.status_changed();
    }
}

/// This build's mDNS service type and the port it listens on: release,
/// preview and dev builds never meet, and each has a port a firewall rule
/// can name.
fn identity() -> (&'static str, u16) {
    match app_brand::SLUG {
        "ficelle" => ("_ficelle._tcp.local.", 47_821),
        "ficelle-preview" => ("_ficelle-preview._tcp.local.", 47_823),
        _ => ("_ficelle-dev._tcp.local.", 47_822),
    }
}

impl<H: Host> Node<H> {
    /// A node, not running yet: see [`Self::set_wanted`].
    pub fn new(host: Arc<H>, me: Me) -> Self {
        let (service, port) = identity();
        Self::with_port(host, me, service, port)
    }

    /// As [`Self::new`] with no discovery at all, as on a network that drops
    /// multicast: paired devices are only reached at the addresses kept.
    /// For tests.
    pub fn without_discovery(host: Arc<H>, me: Me) -> Self {
        Self::with_port(host, me, "", identity().1)
    }

    /// A node on a service type and port of its own, for tests. An empty
    /// service type turns discovery off.
    pub fn with_port(host: Arc<H>, me: Me, service: &str, preferred_port: u16) -> Self {
        Self {
            shared: Arc::new(Shared {
                host,
                me,
                state: Mutex::new(State::default()),
                wake: Condvar::new(),
                preferred_port,
                service: service.to_owned(),
            }),
        }
    }

    fn shared(&self) -> Arc<Shared<H>> {
        self.shared.clone()
    }

    /// Whether this device wants local sync: the vault is open, and on a
    /// phone, the app is in front. It runs while wanted and a device is
    /// paired or pairing is open.
    pub fn set_wanted(&self, wanted: bool) {
        let shared = self.shared();
        shared.lock().wanted = wanted;
        self.refresh();
    }

    /// Checks whether to run, after something that could change it.
    pub fn refresh(&self) {
        self.shared.refresh();
    }

    /// A round with every connected device now, and a new try for the
    /// others.
    pub fn sync_now(&self) {
        let shared = self.shared();
        let mut state = shared.lock();
        for slot in state.sessions.values_mut() {
            slot.now = true;
        }
        state.cooldown.clear();
        state.live.retain(|_, l| !matches!(l, Live::Failed { .. }));
        drop(state);
        shared.wake.notify_all();
    }

    /// Asks the connected devices for a file opened here, whatever its
    /// size. False when none is connected.
    pub fn want(&self, sha256: &str) -> bool {
        let shared = self.shared();
        let mut state = shared.lock();
        for slot in state.sessions.values_mut() {
            slot.wants.push(sha256.to_owned());
        }
        !state.sessions.is_empty()
    }

    /// Ends the session with a device just forgotten.
    pub fn forget(&self, key: &[u8]) {
        let shared = self.shared();
        if let Ok(key) = <[u8; 32]>::try_from(key) {
            let mut state = shared.lock();
            if let Some(slot) = state.sessions.get_mut(&key) {
                slot.stop = true;
            }
            state.live.remove(&key);
            state.unreachable.remove(&key);
        }
        self.refresh();
    }

    /// Shows a code for another device to type.
    ///
    /// # Errors
    ///
    /// The vault refuses pairing (a server is linked) or is locked.
    pub fn start_pairing(&self) -> Result<PairingStatus, Error> {
        let shared = self.shared();
        store(&*shared.host, |s, _| s.peer_check_pairing())?;
        shared.load_identity()?;
        let code = six_digits()?;
        shared.lock().pairing = Some(Pairing {
            code,
            until: Instant::now() + CODE_FOR,
            tries: TRIES,
            busy: false,
            #[cfg(test)]
            checked: 0,
            failed: None,
            paired: None,
        });
        self.refresh();
        self.status()
            .pairing
            .ok_or_else(|| Error::Io(std::io::Error::other("pairing did not start")))
    }

    /// Takes the code away.
    pub fn cancel_pairing(&self) {
        self.shared().lock().pairing = None;
        self.refresh();
    }

    /// Pairs with the device showing `code`, found by discovery or at
    /// `address`. Blocks until done. The paired device's name.
    ///
    /// # Errors
    ///
    /// [`Error::WrongCode`], [`Error::NotFound`] when no device shows a
    /// code, the vault refuses pairing.
    pub fn pair_with_code(&self, code: &str, address: Option<&str>) -> Result<String, Error> {
        let code: String = code.chars().filter(char::is_ascii_digit).collect();
        if code.len() != 6 {
            return Err(Error::WrongCode);
        }
        let shared = self.shared();
        store(&*shared.host, |s, _| s.peer_check_pairing())?;
        shared.load_identity()?;
        let typed = match address {
            Some(text) => Some(
                parse_address(text, shared.preferred_port)
                    .ok_or(Error::Protocol("an address that does not read"))?,
            ),
            None => None,
        };
        shared.lock().typing = true;
        self.refresh();
        let outcome = self.try_code(&shared, &code, typed);
        shared.lock().typing = false;
        self.refresh();
        outcome
    }

    fn try_code(
        &self,
        shared: &Arc<Shared<H>>,
        code: &str,
        typed: Option<SocketAddr>,
    ) -> Result<String, Error> {
        let secret = shared.secret().ok_or(Error::Locked)?;
        let started = Instant::now();
        let mut tried: HashSet<SocketAddr> = HashSet::new();
        let mut wrong = false;
        loop {
            let candidates: Vec<SocketAddr> = if let Some(address) = typed {
                vec![address]
            } else {
                let state = shared.lock();
                let own = state.running.as_ref().map(|r| r.instance.clone());
                state
                    .found
                    .iter()
                    .filter(|(name, f)| {
                        f.pairing && !own.as_deref().is_some_and(|own| name.starts_with(own))
                    })
                    .flat_map(|(_, f)| f.addresses.iter().copied())
                    .collect()
            };
            for address in candidates {
                if !tried.insert(address) {
                    continue;
                }
                let Ok(stream) = TcpStream::connect_timeout(&address, CONNECT) else {
                    continue;
                };
                let mut ch = match channel::dial_pairing(stream, &secret, code) {
                    Ok(ch) => ch,
                    // the side showing the code hangs up on a wrong one
                    Err(Error::Handshake(_) | Error::Closed | Error::Timeout | Error::Io(_)) => {
                        wrong = true;
                        continue;
                    }
                    Err(_) => continue,
                };
                match shared.pair(&mut ch) {
                    Ok(name) => {
                        self.refresh();
                        shared.start_session(ch, Some(address), true);
                        return Ok(name);
                    }
                    Err(Error::WrongCode) => wrong = true,
                    Err(other) => return Err(other),
                }
            }
            if typed.is_some() || started.elapsed() > LOOK_FOR {
                return Err(if wrong {
                    Error::WrongCode
                } else {
                    Error::NotFound
                });
            }
            std::thread::sleep(TICK);
        }
    }

    /// Where things stand.
    pub fn status(&self) -> Status {
        let shared = self.shared();
        let mut state = shared.lock();
        let now = Instant::now();
        let address = state
            .running
            .as_ref()
            .and_then(|r| own_ip().map(|ip| format!("{ip}:{}", r.port)));
        if let Some(p) = state.pairing.as_mut()
            && p.failed.is_none()
            && p.paired.is_none()
            && now >= p.until
        {
            p.failed = Some("expired");
        }
        let pairing = state.pairing.as_ref().map(|p| {
            let open = p.failed.is_none() && p.paired.is_none();
            PairingStatus {
                code: if open { p.code.clone() } else { String::new() },
                seconds_left: p.until.saturating_duration_since(now).as_secs(),
                address: address.clone(),
                failed: p.failed,
                paired: p.paired.clone(),
            }
        });
        Status {
            running: state.running.is_some(),
            port: state.running.as_ref().map(|r| r.port),
            discovery_error: state.discovery_error.clone(),
            pairing,
            live: state
                .live
                .iter()
                .map(|(k, v)| (hex(k), v.clone()))
                .chain(
                    state
                        .sessions
                        .keys()
                        .filter(|k| !state.live.contains_key(*k))
                        .map(|k| (hex(k), Live::Syncing)),
                )
                .collect(),
            blocked: state
                .unreachable
                .iter()
                .filter(|(k, since)| {
                    !state.sessions.contains_key(*k) && now.duration_since(**since) >= BLOCKED_AFTER
                })
                .map(|(k, _)| hex(k))
                .collect(),
        }
    }
}
