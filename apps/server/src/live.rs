// SPDX-License-Identifier: AGPL-3.0-or-later

//! Who is listening for changes: one broadcast channel per account with
//! devices connected, made on first use and dropped with its last listener.
//! A push tells the account's channel, the live sockets pass it on, devices
//! pull. Notices carry no content, only that there is something to pull.

use std::collections::HashMap;
use std::sync::Mutex;

use tokio::sync::broadcast;

/// Notices kept for a slow socket before it skips ahead. A skipped notice
/// costs nothing: the next one, or the lag itself, makes the device pull.
const BACKLOG: usize = 64;

/// Something happened that connected devices should hear about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    /// A device pushed changes to the account's space.
    Changed {
        /// Number of the latest change now.
        head: i64,
        /// The device that pushed, it already has them.
        device: String,
    },
    /// The admin turned co-editing on or off.
    Settings {
        /// The new setting.
        coediting: bool,
    },
}

/// The channels, shared by every request.
#[derive(Debug, Default)]
pub struct Live {
    spaces: Mutex<HashMap<String, broadcast::Sender<Notice>>>,
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
    pub fn changed(&self, account: &str, head: i64, device: &str) {
        let mut accounts = self.lock();
        if let Some(sender) = accounts.get(account) {
            if sender.receiver_count() == 0 {
                accounts.remove(account);
            } else {
                let _ = sender.send(Notice::Changed {
                    head,
                    device: device.to_owned(),
                });
            }
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
        live.changed("a", 3, "phone");
        assert_eq!(
            a.try_recv().unwrap(),
            Notice::Changed {
                head: 3,
                device: "phone".into(),
            }
        );
        assert!(b.try_recv().is_err());
        live.settings(false);
        assert_eq!(a.try_recv().unwrap(), Notice::Settings { coediting: false });
        assert_eq!(b.try_recv().unwrap(), Notice::Settings { coediting: false });
        drop(b);
        live.changed("b", 1, "x");
        assert_eq!(live.listening(), 1, "a channel without listeners goes");
        live.changed("nobody", 1, "x");
    }
}
