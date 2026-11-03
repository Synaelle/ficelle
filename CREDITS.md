# Credits

## Maker

[Synaelle](https://github.com/Synaelle) makes Ficelle.

## Contributors

No one else has contributed code yet. Contributions are welcome, and every contributor is credited here.

## What Ficelle builds on

Ficelle stands on the work of the projects below, with thanks. Each is listed with what Ficelle uses it for and the license it is under. The same list is in the app, under Settings and About.

### App and editor

- [Tauri](https://tauri.app/), the app on desktop and phone. Apache-2.0 or MIT.
- [Tauri plugins](https://github.com/tauri-apps/plugins-workspace), clipboard, dialogs, files, shortcuts, vibration and fingerprint. Apache-2.0 or MIT.
- [Svelte](https://svelte.dev/), the interface. MIT.
- [Tiptap](https://tiptap.dev/), the note editor. MIT.
- [ProseMirror](https://prosemirror.net/), the editing engine under Tiptap. MIT.
- [marked](https://marked.js.org/), reading Markdown. MIT.
- [highlight.js](https://highlightjs.org/), colors in code blocks. BSD-3-Clause.
- [lowlight](https://github.com/wooorm/lowlight), code blocks in the editor. MIT.
- [modern-screenshot](https://github.com/qq15725/modern-screenshot), the picture in an error report. MIT.
- [fontdb](https://github.com/RazrFalcon/fontdb), listing the fonts on the device. MIT.
- [windows](https://github.com/microsoft/windows-rs), Windows system features. MIT or Apache-2.0.
- [winsafe](https://github.com/rodrigocfd/winsafe), Windows system features. MIT.
- [serde and serde_json](https://serde.rs/), reading and writing data. MIT or Apache-2.0.
- [thiserror](https://github.com/dtolnay/thiserror), error types. MIT or Apache-2.0.
- [uuid](https://github.com/uuid-rs/uuid), note and folder ids. Apache-2.0 or MIT.

### Storage and sync

- [Loro](https://loro.dev/), merging edits made on several devices. MIT.
- [rusqlite](https://github.com/rusqlite/rusqlite), the database from Rust. MIT.
- [SQLCipher](https://www.zetetic.net/sqlcipher/), the encrypted database. BSD-style.
- [SQLite](https://sqlite.org/), the database engine. Public domain.
- [ureq](https://github.com/algesten/ureq), talking to S3, WebDAV and updates. MIT or Apache-2.0.
- [tungstenite](https://github.com/snapview/tungstenite-rs), live sync connections. MIT or Apache-2.0.
- [flate2](https://github.com/rust-lang/flate2-rs), compression. MIT or Apache-2.0.
- [tar](https://github.com/composefs/tar-rs), backup archives. MIT or Apache-2.0.
- [zip](https://github.com/zip-rs/zip2), ZIP exports and imports. MIT.
- [jiff](https://github.com/BurntSushi/jiff), dates and times. Unlicense or MIT.
- [tempfile](https://github.com/Stebalien/tempfile), safe temporary files. MIT or Apache-2.0.

### Security

- [argon2](https://github.com/RustCrypto/password-hashes), turning a password into a key. MIT or Apache-2.0.
- [chacha20poly1305](https://github.com/RustCrypto/AEADs), encrypting notes and backups. Apache-2.0 or MIT.
- [aead-stream](https://github.com/RustCrypto/AEADs), encrypting large files in parts. Apache-2.0 or MIT.
- [hkdf, hmac, sha1 and sha2](https://github.com/RustCrypto), key derivation and hashes. MIT or Apache-2.0.
- [hpke](https://github.com/rozbb/rust-hpke), sharing keys between people. MIT or Apache-2.0.
- [opaque-ke](https://github.com/facebook/opaque-ke), signing in without sending the password. Apache-2.0 or MIT.
- [ed25519-dalek](https://github.com/dalek-cryptography/curve25519-dalek), checking signatures. BSD-3-Clause.
- [minisign-verify](https://github.com/jedisct1/rust-minisign-verify), checking app updates. MIT.
- [rand and getrandom](https://github.com/rust-random), random numbers for keys. MIT or Apache-2.0.
- [zeroize](https://github.com/RustCrypto/utils), wiping keys from memory. Apache-2.0 or MIT.
- [keyring](https://github.com/open-source-cooperative/keyring-rs), the system keychain. MIT or Apache-2.0.
- [base64](https://github.com/marshallpierce/rust-base64), text forms of keys and data. MIT or Apache-2.0.
- [OpenSSL](https://www.openssl.org/), under SQLCipher. Apache-2.0.
- [wasm-bindgen](https://wasm-bindgen.github.io/wasm-bindgen/), the encryption in the web page. MIT or Apache-2.0.

### Server

- [axum](https://github.com/tokio-rs/axum), the web server. MIT.
- [tokio](https://tokio.rs/), running many things at once. MIT.
- [tracing and tracing-subscriber](https://github.com/tokio-rs/tracing), logs. MIT.
- [russh and russh-sftp](https://github.com/warp-tech/russh), SFTP backups. Apache-2.0.
- [lettre](https://lettre.rs/), sending email. MIT.
- [openidconnect](https://github.com/ramosbugs/openidconnect-rs), signing in with another service. MIT.
- [webauthn-rs](https://github.com/kanidm/webauthn-rs), passkeys. MPL-2.0.
- [reqwest](https://github.com/seanmonstar/reqwest), fetching the official catalog. MIT or Apache-2.0.
- [qrcode](https://github.com/kennytm/qrcode-rust), QR codes. MIT or Apache-2.0.
- [toml](https://github.com/toml-rs/toml), reading the settings file. MIT or Apache-2.0.
- [url](https://github.com/servo/rust-url), web addresses. MIT or Apache-2.0.
- [data-encoding](https://github.com/ia0/data-encoding), text forms of binary data. MIT.
- [futures-core](https://github.com/rust-lang/futures-rs), asynchronous streams. MIT or Apache-2.0.
- [rpassword](https://github.com/conradkleinespel/rpassword), typing a password in the terminal. Apache-2.0.

### Fonts and icons

- [Hanken Grotesk](https://fontsource.org/fonts/hanken-grotesk), the default font. OFL-1.1.
- [Atkinson Hyperlegible Next](https://fontsource.org/fonts/atkinson-hyperlegible-next), an easy to read font. OFL-1.1.
- [Noto Sans Math](https://fontsource.org/fonts/noto-sans-math), the arrows. OFL-1.1.
- [Noto Sans Symbols 2](https://fontsource.org/fonts/noto-sans-symbols-2), more arrows. OFL-1.1.
- [Schibsted Grotesk](https://fontsource.org/fonts/schibsted-grotesk), the Carbon theme. OFL-1.1.
- [Literata](https://fontsource.org/fonts/literata), the Classic theme. OFL-1.1.
- [IBM Plex Mono](https://fontsource.org/fonts/ibm-plex-mono), the code font. OFL-1.1.
- [Ubuntu and Ubuntu Mono](https://fontsource.org/fonts/ubuntu), a theme font. UFL-1.0.
- [OpenDyslexic](https://opendyslexic.org/), the dyslexia friendly font, by Abbie Gonzalez. OFL-1.1.
- [Fontsource](https://fontsource.org/), the fonts offered for notes and code. Each font under its own license, mostly OFL-1.1.
- [Lucide](https://lucide.dev/), the icons. ISC.
- [Phosphor Icons](https://phosphoricons.com/), a second icon set. MIT.
