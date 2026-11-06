# Ficelle on a Mac

The Mac app is untested, I have no Mac to try it on, so keep backups of your notes and tell me what goes wrong. The betas no longer include it. It gets one release once the current rework is done, then only major security or bug fixes, until it can be tested daily. Until that release, the latest Mac build is 0.5.0-beta.2.

Download the `.dmg` from the [releases](https://github.com/Synaelle/ficelle/releases), the same file for Apple Silicon and Intel, open it and drag Ficelle to Applications. From beta 2 on, the app updates itself when run from there; otherwise it opens the release page when a new version is out. This has not been tried on a real Mac yet.

The app is not signed by Apple, so macOS refuses to open it the first time. Click Done, then open System Settings, Privacy & Security, scroll down and click Open Anyway next to Ficelle. Confirm with your password and open Ficelle again.

If macOS says the app is damaged, or Open Anyway does not show, run this once in Terminal, then open Ficelle.

```sh
xattr -dr com.apple.quarantine /Applications/Ficelle.app
```

## Help with testing

You are more than welcome to help. [Open an issue](https://github.com/Synaelle/ficelle/issues) or write to synaelle@protonmail.com, and tell how a build went, even if it does not start.
