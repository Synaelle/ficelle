# Ficelle on a Mac or an iPhone

The Mac app is coming soon with the Beta releases. A web version is on its way too, and will be the recommended way to use Ficelle on Apple devices. Until then, [Obsidian](https://obsidian.md) on macOS and iOS opens Markdown notes, and Ficelle's Export as markdown (Settings, Data and backups) opens in it.

## Mac

Once the Mac app is out, download the `.dmg` for your Mac (Apple Silicon for M1 and later, or Intel), open it and drag Ficelle to Applications.

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
