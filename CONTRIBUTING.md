# Contributing to Ficelle

Ficelle is made by one person in their spare time, so every bit of help counts. Thank you for looking.

## Languages

English is the reference language and the fallback, so a text that is not translated yet shows in English. Each language is one file in [packages/locales](packages/locales), and every change ends up there, whichever way you send it.

### On Weblate

Coming soon. Weblate will let you translate in the browser, with nothing to install, and it will stay in sync with the files here on its own.

### On GitHub

You can also edit a file right on GitHub, without git or any tool.

1. Open the language's file in [packages/locales](packages/locales), for example `fr.toml` for French.
2. Click the pencil icon at the top right of the file. GitHub makes your own copy of the project the first time.
3. Change the texts between the quotes. Keep the keys on the left as they are, and keep words in `{braces}` exactly as they are, the app fills them in.
4. Click Commit changes, then Propose changes, then Create pull request.

I check the change and merge it.

To add a language, open an issue or a pull request with a copy of `en.toml` named after the language's code (`de.toml` for German). The app needs a small change to offer it, I take care of that.

## Code

Bug fixes, security fixes and improvements to the app and its interface are welcome. Please open an issue first for anything bigger than a small fix, so the approach is settled before you spend time on it.

New features need to be discussed first in [Discussions](../../discussions). Ficelle tries to stay small and simple, and a feature agreed on beforehand is much more likely to be merged.

To report a security problem, use [Report a vulnerability](../../security/advisories/new) instead of a public issue.

[Building Ficelle](docs/building.md) explains how to build and run the app. Before opening a pull request, run `pnpm lint`, `cargo clippy --workspace --all-targets -- -D warnings` and the tests.

### Wanted most, iOS and macOS

The thing Ficelle needs most is someone with a Mac or an iPhone. I don't have the hardware, so the Mac app is built but untested, and there is no iOS app at all. If you can build, test or develop for Apple's platforms, please get in touch in [Discussions](../../discussions).
