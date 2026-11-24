# @app/locales

One TOML file per language, English and French, with English as the fallback. See [Contributing](../../CONTRIBUTING.md#languages).

The admin page and the server's mails are in English only, in `server/texts.toml`. Only the admin page loads it, so the apps never carry these texts.

Keep each section's tables together (`[list]` then `[list.count]`), or Weblate can't read the file. `pnpm lint` checks keys, plural forms and placeholders.
