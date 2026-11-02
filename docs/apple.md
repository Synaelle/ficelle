# Ficelle on a Mac or an iPhone

> [!CAUTION]
> These builds are untested, may not start, and do not follow Ficelle's version numbers. Do not use them for anything serious, keep your real notes elsewhere.

Get them from [Apple test builds](https://github.com/Synaelle/ficelle/releases/tag/apple-test). A web version is on its way and will be the recommended way to use Ficelle on Apple devices. Until then, [Obsidian](https://obsidian.md) on macOS and iOS opens Markdown notes, and Ficelle's Export as markdown (Settings, Data and backups) opens in it.

## Mac

Open `ficelle-apple-test-macos-arm.dmg` on a Mac with Apple Silicon (M1 and later), or `ficelle-apple-test-macos-intel.dmg` on an Intel Mac, and drag Ficelle to Applications. It is not signed by Apple, so the first time right-click it, choose Open, then Open again. If macOS says it is damaged, run this once in Terminal.

```sh
xattr -dr com.apple.quarantine /Applications/Ficelle.app
```

## iPhone and iPad

Install `ficelle-apple-test-ios.ipa` with [AltStore](https://altstore.io) or [SideStore](https://sidestore.io), which sign it with your own Apple ID. With a free Apple ID it needs a refresh every 7 days.

## Help with testing

You are more than welcome to help. [Open an issue](https://github.com/Synaelle/ficelle/issues) or write to synaelle@protonmail.com, and tell how a build went, even if it does not start.
