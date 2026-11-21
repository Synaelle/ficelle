# @app/locales

One TOML file per language, with English as the fallback. `tools/deepl.mjs` fills in missing German, Spanish, Italian, Dutch and Portuguese texts. See [Contributing](../../CONTRIBUTING.md#languages).

Keep each section's tables together (`[list]` then `[list.count]`), or Weblate can't read the file. `pnpm lint` checks keys, plural forms and placeholders.
