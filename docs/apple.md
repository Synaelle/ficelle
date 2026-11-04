# Ficelle on a Mac or an iPhone

The Mac app is in 0.4.0, then it comes with the Beta releases, early versions of the next release. It is untested, I have no Mac to try it on, so keep backups of your notes and tell me what goes wrong. On iOS, [Obsidian](https://obsidian.md) opens Markdown notes, and Ficelle's Export as markdown (Settings, Data and backups) opens in it.

## Mac

Download the `.dmg` from the [releases](https://github.com/Synaelle/ficelle/releases), the same file for Apple Silicon and Intel, open it and drag Ficelle to Applications. From beta 2 on, the app updates itself when run from there; otherwise it opens the release page when a new version is out. This has not been tried on a real Mac yet.

The app is not signed by Apple, so macOS refuses to open it the first time. Click Done, then open System Settings, Privacy & Security, scroll down and click Open Anyway next to Ficelle. Confirm with your password and open Ficelle again.

If macOS says the app is damaged, or Open Anyway does not show, run this once in Terminal, then open Ficelle.

```sh
xattr -dr com.apple.quarantine /Applications/Ficelle.app
```

## iPhone and iPad

> [!CAUTION]
> The iOS build is untested, may not start, and does not follow Ficelle's version numbers. Do not use it for anything serious, keep your real notes elsewhere.

Get `ficelle-apple-test-ios.ipa` from the [iOS test build](https://github.com/Synaelle/ficelle/releases/tag/apple-test) and install it with [AltStore](https://altstore.io) or [SideStore](https://sidestore.io), which sign it with your own Apple ID. With a free Apple ID it needs a refresh every 7 days.

## Help with testing

You are more than welcome to help. [Open an issue](https://github.com/Synaelle/ficelle/issues) or write to synaelle@protonmail.com, and tell how a build went, even if it does not start.
