# @app/locales

One file per language, English is the fallback. Texts are TOML grouped by screen, with `{name}` placeholders and plural forms chosen from `{count}`.

Languages other than English are machine translated. To correct a text, edit its line in the language's file and open a pull request. To add a language, copy en.toml to the language's code (fr.toml for French), translate every value and keep the keys and placeholders as they are. `pnpm lint` reports missing keys and placeholders that differ from English.
