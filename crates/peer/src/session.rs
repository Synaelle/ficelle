// SPDX-License-Identifier: AGPL-3.0-or-later

//! One connection with a paired device: hellos, device lists, then rounds
//! of versions and changes, and files, until one side closes.

use std::time::{Duration, Instant};

use app_core::blobs::sha256_hex;
use app_core::store::PeerDevice;
use app_protocol::local::{DATA_FORMAT, MIN_PROTOCOL, PROTOCOL};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;

use crate::channel::Channel;
use crate::node::{Live, Shared};
use crate::wire::{self, Device, Doc, Incoming, Msg};
use crate::{Error, Host, store};

/// Changes per message, about.
const BATCH: usize = 4 * 1024 * 1024;
/// A quiet connection says it is still there this often.
const PING_EVERY: Duration = Duration::from_secs(30);
/// Nothing at all for this long and the other side is gone.
const SILENT_FOR: Duration = Duration::from_secs(75);
/// Writes come in bursts while typing: a round waits for this much quiet.
pub(crate) const SETTLE: Duration = Duration::from_millis(1500);

/// The other side's hello.
pub(crate) struct Hello {
    pub name: String,
    pub platform: String,
    pub version: String,
    pub min_protocol: u32,
    pub protocol: u32,
    pub format: u32,
    pub port: Option<u16>,
}

/// Sends this device's hello and reads the other's.
pub(crate) fn hellos<H: Host>(shared: &Shared<H>, ch: &mut Channel) -> Result<Hello, Error> {
    let name =
        store(&*shared.host, |s, _| s.peer_name())?.unwrap_or_else(|| shared.host.default_name());
    ch.send(&wire::encode(&Msg::Hello {
        name,
        platform: shared.me.platform.clone(),
        version: shared.me.version.clone(),
        min_protocol: MIN_PROTOCOL,
        protocol: PROTOCOL,
        format: DATA_FORMAT,
        port: shared.port(),
    }))?;
    match wire::decode(&ch.recv()?)? {
        Incoming::Msg(Msg::Hello {
            name,
            platform,
            version,
            min_protocol,
            protocol,
            format,
            port,
        }) => Ok(Hello {
            name,
            platform,
            version,
            min_protocol,
            protocol,
            format,
            port,
        }),
        Incoming::Msg(Msg::Bye { reason }) if reason == "server" => Err(Error::Handshake(
            "the other device syncs with a server".into(),
        )),
        _ => Err(Error::Protocol("no hello")),
    }
}

/// Whether two devices can sync, from the other's hello: `None` when they
/// can, else who has to update.
pub(crate) fn mismatch(hello: &Hello) -> Option<Live> {
    let they_older = hello.protocol < MIN_PROTOCOL || hello.format < DATA_FORMAT;
    let me_older = PROTOCOL < hello.min_protocol || DATA_FORMAT < hello.format;
    if they_older {
        Some(Live::NeedsUpdate {
            version: hello.version.clone(),
        })
    } else if me_older {
        Some(Live::UpdateThis {
            version: hello.version.clone(),
        })
    } else {
        None
    }
}

fn device_list(devices: &[PeerDevice]) -> Vec<Device> {
    devices
        .iter()
        .map(|d| Device {
            key: STANDARD.encode(&d.key),
            name: d.name.clone(),
            platform: d.platform.clone(),
            paired_at: d.paired_at,
            forgotten_at: d.forgotten_at,
        })
        .collect()
}

fn learned(devices: Vec<Device>) -> Vec<PeerDevice> {
    devices
        .into_iter()
        .filter_map(|d| {
            Some(PeerDevice {
                key: STANDARD.decode(d.key).ok().filter(|k| k.len() == 32)?,
                name: d.name,
                platform: d.platform,
                version: String::new(),
                address: None,
                paired_at: d.paired_at,
                last_sync_at: None,
                forgotten_at: d.forgotten_at,
            })
        })
        .collect()
}

struct Session<'a, H: Host> {
    shared: &'a Shared<H>,
    ch: Channel,
    key: [u8; 32],
    id: u64,
    last_heard: Instant,
    last_said: Instant,
}

impl<H: Host> Session<'_, H> {
    fn say(&mut self, msg: &Msg) -> Result<(), Error> {
        self.ch.send(&wire::encode(msg))?;
        self.last_said = Instant::now();
        Ok(())
    }

    fn versions(&mut self, answer: bool) -> Result<(), Error> {
        let docs = store(&*self.shared.host, |s, _| s.peer_versions())?;
        let docs = docs.into_iter().map(|(id, b)| Doc::new(id, &b)).collect();
        self.say(&Msg::Versions { docs, answer })
    }

    fn changes_for(&mut self, theirs: &[(String, Vec<u8>)]) -> Result<(), Error> {
        let changes = store(&*self.shared.host, |s, _| s.peer_changes(theirs))?;
        let mut batch = Vec::new();
        let mut size = 0;
        for (id, bytes) in changes {
            size += bytes.len();
            batch.push(Doc::new(id, &bytes));
            if size >= BATCH {
                self.say(&Msg::Changes {
                    docs: std::mem::take(&mut batch),
                    last: false,
                })?;
                size = 0;
            }
        }
        self.say(&Msg::Changes {
            docs: batch,
            last: true,
        })
    }

    /// Files the notes here use and this device lacks, small enough.
    fn wanted(&self) -> Result<Vec<String>, Error> {
        let limit = self.shared.host.fetch_up_to();
        store(&*self.shared.host, |s, blobs| {
            let mut wanted = Vec::new();
            for file in s.files_in_use()? {
                let small = file
                    .size
                    .is_none_or(|n| u64::try_from(n).unwrap_or(u64::MAX) <= limit);
                if file.space.is_empty() && small && !blobs.has(&file.sha256)? {
                    wanted.push(file.sha256);
                }
            }
            wanted.dedup();
            Ok(wanted)
        })
    }

    fn handle(&mut self, incoming: Incoming) -> Result<bool, Error> {
        match incoming {
            Incoming::Msg(Msg::Versions { docs, answer }) => {
                let theirs = wire::decode_docs(docs)?;
                self.changes_for(&theirs)?;
                if answer {
                    self.versions(false)?;
                }
            }
            Incoming::Msg(Msg::Changes { docs, last }) => {
                let changes = wire::decode_docs(docs)?;
                let taken = store(&*self.shared.host, |s, _| s.peer_take(&changes))?;
                if taken > 0 {
                    self.shared.host.changed();
                }
                if last {
                    let wanted = self.wanted()?;
                    if !wanted.is_empty() {
                        self.say(&Msg::Want { files: wanted })?;
                    }
                    let key = self.key;
                    store(&*self.shared.host, |s, _| s.peer_synced(&key))?;
                    self.shared.set_live(&self.key, Live::Synced);
                }
            }
            Incoming::Msg(Msg::Want { files }) => {
                for sha256 in files {
                    let bytes = store(&*self.shared.host, |_, blobs| blobs.get(&sha256))?;
                    match bytes {
                        Some(bytes) => {
                            self.ch.send(&wire::encode_file(&sha256, &bytes))?;
                            self.last_said = Instant::now();
                        }
                        None => self.say(&Msg::Missing { sha256 })?,
                    }
                }
            }
            Incoming::File { sha256, bytes } => {
                // kept only when the bytes are the file asked for
                if sha256_hex(&bytes) == sha256 {
                    store(&*self.shared.host, |_, blobs| blobs.put(&bytes))?;
                    self.shared.host.files_arrived();
                }
            }
            Incoming::Msg(Msg::Missing { .. } | Msg::Pong | Msg::Hello { .. }) => {}
            Incoming::Msg(Msg::Devices { devices }) => self.learn(devices)?,
            Incoming::Msg(Msg::Ping) => self.say(&Msg::Pong)?,
            Incoming::Msg(Msg::Bye { .. }) => return Ok(false),
        }
        Ok(true)
    }

    fn learn(&mut self, devices: Vec<Device>) -> Result<(), Error> {
        let own = self.shared.public();
        let list = learned(devices);
        if store(&*self.shared.host, |s, _| s.peer_learn(&list, &own))? {
            self.shared.host.status_changed();
        }
        Ok(())
    }

    /// Rounds until one side closes or this session is told to stop.
    fn run(&mut self, starts: bool) -> Result<(), Error> {
        let devices = store(&*self.shared.host, |s, _| s.peer_devices())?;
        self.say(&Msg::Devices {
            devices: device_list(&devices),
        })?;
        if starts {
            self.shared.set_live(&self.key, Live::Syncing);
            self.versions(true)?;
        }
        loop {
            if !self.shared.session_wanted(&self.key, self.id) {
                let _ = self.say(&Msg::Bye {
                    reason: "stopped".into(),
                });
                return Ok(());
            }
            if self.shared.take_kick(&self.key, self.id) {
                self.shared.set_live(&self.key, Live::Syncing);
                self.versions(true)?;
            }
            let wants = self.shared.take_wants(&self.key, self.id);
            if !wants.is_empty() {
                self.say(&Msg::Want { files: wants })?;
            }
            if self.last_said.elapsed() > PING_EVERY {
                self.say(&Msg::Ping)?;
            }
            match self.ch.poll()? {
                Some(message) => {
                    self.last_heard = Instant::now();
                    if !self.handle(wire::decode(&message)?)? {
                        return Ok(());
                    }
                }
                None if self.last_heard.elapsed() > SILENT_FOR => return Err(Error::Timeout),
                None => {}
            }
        }
    }
}

/// Runs a session on a channel whose hellos were said. Tells the node when
/// it ends.
pub(crate) fn run<H: Host>(shared: &Shared<H>, ch: Channel, id: u64, starts: bool) {
    let key = ch.remote_key;
    let mut session = Session {
        shared,
        ch,
        key,
        id,
        last_heard: Instant::now(),
        last_said: Instant::now(),
    };
    let outcome = session.run(starts);
    session.ch.close();
    shared.ended(&key, id, outcome.err());
}

/// After the hellos: refuses a device on another version, else notes what
/// it said. `Err` with the state to show when the two cannot sync.
/// The address it answers on is kept, to dial it there when discovery
/// does not find it (multicast blocked): the one this device dialled, or,
/// when the other dialled in, its address with the port its hello gave.
pub(crate) fn check_hello<H: Host>(
    shared: &Shared<H>,
    ch: &mut Channel,
    hello: &Hello,
    dialled: Option<std::net::SocketAddr>,
) -> Result<(), Live> {
    let key = ch.remote_key;
    let address = dialled
        .or_else(|| {
            let port = hello.port?;
            ch.peer_address()
                .map(|a| std::net::SocketAddr::new(a.ip(), port))
        })
        .map(|a| a.to_string());
    let _ = store(&*shared.host, |s, _| {
        s.peer_seen(
            &key,
            &hello.name,
            &hello.platform,
            &hello.version,
            address.as_deref(),
            false,
        )
    });
    if let Some(live) = mismatch(hello) {
        let _ = ch.send(&wire::encode(&Msg::Bye {
            reason: "version".into(),
        }));
        return Err(live);
    }
    Ok(())
}
