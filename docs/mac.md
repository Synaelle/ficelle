# Ficelle on a Mac

On a Mac, the [web version](https://ficelle.synaelle.com/web/) is recommended. Your notes stay in your browser on that Mac.

The Mac app is no longer made, because I have no Mac to test it on. Its last version, 0.7.1, can still be downloaded, but it was never tried on a real Mac and gets no updates. Keep backups of your notes if you use it.

Download [`Ficelle_0.7.1_universal.dmg`](https://github.com/Synaelle/ficelle/releases/download/v0.7.1/Ficelle_0.7.1_universal.dmg), the same file for Apple Silicon and Intel, open it and drag Ficelle to Applications.

The app is not signed by Apple, so macOS refuses to open it the first time. Click Done, then open System Settings, Privacy & Security, scroll down and click Open Anyway next to Ficelle. Confirm with your password and open Ficelle again.

If macOS says the app is damaged, or Open Anyway does not show, run this once in Terminal, then open Ficelle.

```sh
xattr -dr com.apple.quarantine /Applications/Ficelle.app
```
