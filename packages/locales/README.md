# @app/locales

One file per language, English is the fallback. Texts are TOML grouped by screen, with `{name}` placeholders and plural forms chosen from `{count}`.

Translations are made on [Weblate](https://hosted.weblate.org/projects/ficelle/) or by pull request, see [Contributing](../../CONTRIBUTING.md#languages). The app finds every file here by itself and offers a language once half of its texts are translated.

Keep each section's tables together, `[list]` and then `[list.count]` with no other section between them, or Weblate cannot read the file. `pnpm lint` reports a split section, missing keys and plural forms, and placeholders that differ from English.
