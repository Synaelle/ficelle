# Contributing to Ficelle

## Languages

Edit a file in [packages/locales](packages/locales) on GitHub and propose the change. Keep the keys and the words in `{braces}` as they are. A language shows in the app once half translated.

Languages with more plural forms than English, such as Polish, Russian or Arabic, list every form they need beside `one` and `other`, as given in the [Unicode plural rules](https://www.unicode.org/cldr/charts/latest/supplemental/language_plural_rules.html).

```toml
[files.inNote]
one = "{count} załącznik"
few = "{count} załączniki"
many = "{count} załączników"
other = "{count} załącznika"
```

## Code

Open an issue before anything bigger than a small fix, and discuss new features in [Discussions](../../discussions) first. Report security problems through [Report a vulnerability](../../security/advisories/new). See [Building Ficelle](docs/building.md), and run `pnpm lint`, `pnpm lint:rust` and `pnpm test` before a pull request.

There is no iOS app because I have no iPhone. Get in touch in Discussions if you can work on one.
