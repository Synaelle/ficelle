# Privacy

Ficelle has no analytics, no tracking and no crash reporting. Your notes stay on your devices, and on a sync server only if you choose one. This page lists everything the apps and the server send by themselves.

## What the apps send

| What             | Where                                                           | When                                                                                                    |
| ---------------- | --------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- |
| Update check     | GitHub, the latest release's `latest.json`                      | Release builds, when the notes are unlocked, every few hours, and when the app comes back after a while |
| Update download  | GitHub, the release files                                       | When a newer version is installed                                                                       |
| Fonts            | ficelle.synaelle.com, or jsDelivr if it fails                   | Once, when you turn on OpenDyslexic, which the app does not carry                                       |
| Feedback         | ficelle.synaelle.com, or your sync server if its admin chose so | Only when you press Send                                                                                |
| Feedback answers | The server that received your feedback                          | When you open Settings, Feedback, after sending one                                                     |

These requests carry no account, no note and no device details. The app names itself as `ficelle`, without a version, except feedback which carries the version. The sites still see your network address, like any visit.

With automatic updates off, the app still reads `latest.json`, only to tell you about a security fix, unless you turn that off too. It downloads nothing without your yes.

## Feedback

A report holds what you write, a contact if you give one, the images you add, the app version and the platform (Windows, Android…). Recent warnings and errors from the app are attached unless you untick them, with folder paths, email addresses and note titles removed as far as the app can find them. The form shows exactly what is sent before you send it.

Each install also sends a random id made once, tied to nothing else, so reports from the same app can be grouped.

ficelle.synaelle.com keeps reports 180 days, unless one is kept to follow a bug. Your network address is never stored, only a keyed fingerprint of it, used against spam and dropped after a day.

## Sync

Sync only talks to the server you sign in to. That server sees your username, your email if you add one, your devices (named after the computer, like "Ficelle on DESKTOP-1234"), and when each one syncs.

What it can read depends on the account.

- With a standard account, notes and files are encrypted on the server's disk, but the server holds the key. Whoever runs the server can read them.
- With an end-to-end encrypted account, notes, titles, folders, tags and files are encrypted on your device. The server cannot read them. It still sees how many notes and files there are, their sizes and when they change. Your password never reaches it.

The server keeps a log of sign-ins, failures and account changes with the network address, 365 days by default. It keeps no log of other requests. Each admin can change these lengths, see their server's privacy page.

## On your devices

Notes and attachments are encrypted on the device once you turn on encryption in Settings, Security. New installs start without it. Settings (appearance, language) are kept unencrypted, they hold nothing about your notes. Warnings and errors stay in memory for feedback and are never written to disk.

Backups are encrypted when your notes are, see [Backups](backups.md). A Markdown export is always unencrypted.

## What a server sends

- Once a day it reads ficelle.synaelle.com's `releases.json` to check for updates, with no version or details. The admin can turn it off.
- Confirmations, sign-in codes and password notices go out through the admin's mail server, only if one is set up.
- With single sign-on set up, the chosen provider learns you signed in.
- An admin can report a server issue to ficelle.synaelle.com from the admin page, which sends the server's version and address.
