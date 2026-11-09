// SPDX-License-Identifier: AGPL-3.0-or-later
// What Ficelle builds on, shown in About. CREDITS.md at the repository root
// says the same, credits.test.ts keeps the two equal. Licenses are those the
// projects state in their package metadata.

export interface Credit {
  name: string
  /** The project's page. */
  url: string
  /** What Ficelle uses it for, a short phrase. */
  use: string
  license: string
}

export interface CreditGroup {
  title: string
  credits: Credit[]
}

export const MAKER = { name: 'Synaelle', url: 'https://github.com/Synaelle' }

const DUAL = 'Apache-2.0 or MIT'
const DUAL_MIT = 'MIT or Apache-2.0'

export const CREDIT_GROUPS: CreditGroup[] = [
  {
    title: 'App and editor',
    credits: [
      {
        name: 'Tauri',
        url: 'https://tauri.app/',
        use: 'the app on desktop and phone',
        license: DUAL,
      },
      {
        name: 'Tauri plugins',
        url: 'https://github.com/tauri-apps/plugins-workspace',
        use: 'clipboard, dialogs, files, shortcuts, vibration and fingerprint',
        license: DUAL,
      },
      { name: 'Svelte', url: 'https://svelte.dev/', use: 'the interface', license: 'MIT' },
      { name: 'Tiptap', url: 'https://tiptap.dev/', use: 'the note editor', license: 'MIT' },
      {
        name: 'ProseMirror',
        url: 'https://prosemirror.net/',
        use: 'the editing engine under Tiptap',
        license: 'MIT',
      },
      { name: 'marked', url: 'https://marked.js.org/', use: 'reading Markdown', license: 'MIT' },
      {
        name: 'highlight.js',
        url: 'https://highlightjs.org/',
        use: 'colors in code blocks',
        license: 'BSD-3-Clause',
      },
      {
        name: 'lowlight',
        url: 'https://github.com/wooorm/lowlight',
        use: 'code blocks in the editor',
        license: 'MIT',
      },
      {
        name: 'modern-screenshot',
        url: 'https://github.com/qq15725/modern-screenshot',
        use: 'the picture in an error report',
        license: 'MIT',
      },
      {
        name: 'windows',
        url: 'https://github.com/microsoft/windows-rs',
        use: 'Windows system features',
        license: DUAL_MIT,
      },
      {
        name: 'winsafe',
        url: 'https://github.com/rodrigocfd/winsafe',
        use: 'Windows system features',
        license: 'MIT',
      },
      {
        name: 'serde and serde_json',
        url: 'https://serde.rs/',
        use: 'reading and writing data',
        license: DUAL_MIT,
      },
      {
        name: 'thiserror',
        url: 'https://github.com/dtolnay/thiserror',
        use: 'error types',
        license: DUAL_MIT,
      },
      {
        name: 'uuid',
        url: 'https://github.com/uuid-rs/uuid',
        use: 'note and folder ids',
        license: DUAL,
      },
    ],
  },
  {
    title: 'Storage and sync',
    credits: [
      {
        name: 'Loro',
        url: 'https://loro.dev/',
        use: 'merging edits made on several devices',
        license: 'MIT',
      },
      {
        name: 'rusqlite',
        url: 'https://github.com/rusqlite/rusqlite',
        use: 'the database from Rust',
        license: 'MIT',
      },
      {
        name: 'SQLCipher',
        url: 'https://www.zetetic.net/sqlcipher/',
        use: 'the encrypted database',
        license: 'BSD-style',
      },
      {
        name: 'SQLite',
        url: 'https://sqlite.org/',
        use: 'the database engine',
        license: 'Public domain',
      },
      {
        name: 'ureq',
        url: 'https://github.com/algesten/ureq',
        use: 'talking to S3, WebDAV and updates',
        license: DUAL_MIT,
      },
      {
        name: 'tungstenite',
        url: 'https://github.com/snapview/tungstenite-rs',
        use: 'live sync connections',
        license: DUAL_MIT,
      },
      {
        name: 'flate2',
        url: 'https://github.com/rust-lang/flate2-rs',
        use: 'compression',
        license: DUAL_MIT,
      },
      {
        name: 'tar',
        url: 'https://github.com/composefs/tar-rs',
        use: 'backup archives',
        license: DUAL_MIT,
      },
      {
        name: 'zip',
        url: 'https://github.com/zip-rs/zip2',
        use: 'ZIP exports and imports',
        license: 'MIT',
      },
      {
        name: 'jiff',
        url: 'https://github.com/BurntSushi/jiff',
        use: 'dates and times',
        license: 'Unlicense or MIT',
      },
      {
        name: 'tempfile',
        url: 'https://github.com/Stebalien/tempfile',
        use: 'safe temporary files',
        license: DUAL_MIT,
      },
    ],
  },
  {
    title: 'Security',
    credits: [
      {
        name: 'argon2',
        url: 'https://github.com/RustCrypto/password-hashes',
        use: 'turning a password into a key',
        license: DUAL_MIT,
      },
      {
        name: 'chacha20poly1305',
        url: 'https://github.com/RustCrypto/AEADs',
        use: 'encrypting notes and backups',
        license: DUAL,
      },
      {
        name: 'aead-stream',
        url: 'https://github.com/RustCrypto/AEADs',
        use: 'encrypting large files in parts',
        license: DUAL,
      },
      {
        name: 'hkdf, hmac, sha1 and sha2',
        url: 'https://github.com/RustCrypto',
        use: 'key derivation and hashes',
        license: DUAL_MIT,
      },
      {
        name: 'hpke',
        url: 'https://github.com/rozbb/rust-hpke',
        use: 'sharing keys between people',
        license: DUAL_MIT,
      },
      {
        name: 'opaque-ke',
        url: 'https://github.com/facebook/opaque-ke',
        use: 'signing in without sending the password',
        license: DUAL,
      },
      {
        name: 'ed25519-dalek',
        url: 'https://github.com/dalek-cryptography/curve25519-dalek',
        use: 'checking signatures',
        license: 'BSD-3-Clause',
      },
      {
        name: 'minisign-verify',
        url: 'https://github.com/jedisct1/rust-minisign-verify',
        use: 'checking app updates',
        license: 'MIT',
      },
      {
        name: 'rand and getrandom',
        url: 'https://github.com/rust-random',
        use: 'random numbers for keys',
        license: DUAL_MIT,
      },
      {
        name: 'zeroize',
        url: 'https://github.com/RustCrypto/utils',
        use: 'wiping keys from memory',
        license: DUAL,
      },
      {
        name: 'keyring',
        url: 'https://github.com/open-source-cooperative/keyring-rs',
        use: 'the system keychain',
        license: DUAL_MIT,
      },
      {
        name: 'base64',
        url: 'https://github.com/marshallpierce/rust-base64',
        use: 'text forms of keys and data',
        license: DUAL_MIT,
      },
      {
        name: 'OpenSSL',
        url: 'https://www.openssl.org/',
        use: 'under SQLCipher',
        license: 'Apache-2.0',
      },
      {
        name: 'wasm-bindgen',
        url: 'https://wasm-bindgen.github.io/wasm-bindgen/',
        use: 'the encryption in the web page',
        license: DUAL_MIT,
      },
    ],
  },
  {
    title: 'Server',
    credits: [
      {
        name: 'axum',
        url: 'https://github.com/tokio-rs/axum',
        use: 'the web server',
        license: 'MIT',
      },
      {
        name: 'tokio',
        url: 'https://tokio.rs/',
        use: 'running many things at once',
        license: 'MIT',
      },
      {
        name: 'tracing and tracing-subscriber',
        url: 'https://github.com/tokio-rs/tracing',
        use: 'logs',
        license: 'MIT',
      },
      {
        name: 'russh and russh-sftp',
        url: 'https://github.com/warp-tech/russh',
        use: 'SFTP backups',
        license: 'Apache-2.0',
      },
      { name: 'lettre', url: 'https://lettre.rs/', use: 'sending email', license: 'MIT' },
      {
        name: 'openidconnect',
        url: 'https://github.com/ramosbugs/openidconnect-rs',
        use: 'signing in with another service',
        license: 'MIT',
      },
      {
        name: 'webauthn-rs',
        url: 'https://github.com/kanidm/webauthn-rs',
        use: 'passkeys',
        license: 'MPL-2.0',
      },
      {
        name: 'reqwest',
        url: 'https://github.com/seanmonstar/reqwest',
        use: 'update checks, feedback and single sign-on',
        license: DUAL_MIT,
      },
      {
        name: 'qrcode',
        url: 'https://github.com/kennytm/qrcode-rust',
        use: 'QR codes',
        license: DUAL_MIT,
      },
      {
        name: 'toml',
        url: 'https://github.com/toml-rs/toml',
        use: 'reading the settings file',
        license: DUAL_MIT,
      },
      {
        name: 'url',
        url: 'https://github.com/servo/rust-url',
        use: 'web addresses',
        license: DUAL_MIT,
      },
      {
        name: 'data-encoding',
        url: 'https://github.com/ia0/data-encoding',
        use: 'text forms of binary data',
        license: 'MIT',
      },
      {
        name: 'futures-core',
        url: 'https://github.com/rust-lang/futures-rs',
        use: 'asynchronous streams',
        license: DUAL_MIT,
      },
      {
        name: 'rpassword',
        url: 'https://github.com/conradkleinespel/rpassword',
        use: 'typing a password in the terminal',
        license: 'Apache-2.0',
      },
    ],
  },
  {
    title: 'Fonts and icons',
    credits: [
      {
        name: 'Hanken Grotesk',
        url: 'https://fontsource.org/fonts/hanken-grotesk',
        use: 'the default font',
        license: 'OFL-1.1',
      },
      {
        name: 'Atkinson Hyperlegible Next',
        url: 'https://fontsource.org/fonts/atkinson-hyperlegible-next',
        use: 'an easy to read font',
        license: 'OFL-1.1',
      },
      {
        name: 'Noto Sans Math',
        url: 'https://fontsource.org/fonts/noto-sans-math',
        use: 'the arrows',
        license: 'OFL-1.1',
      },
      {
        name: 'Noto Sans Symbols 2',
        url: 'https://fontsource.org/fonts/noto-sans-symbols-2',
        use: 'more arrows',
        license: 'OFL-1.1',
      },
      {
        name: 'IBM Plex Mono',
        url: 'https://fontsource.org/fonts/ibm-plex-mono',
        use: 'the code font',
        license: 'OFL-1.1',
      },
      {
        name: 'OpenDyslexic',
        url: 'https://opendyslexic.org/',
        use: 'the dyslexia friendly font, by Abbie Gonzalez',
        license: 'OFL-1.1',
      },
      {
        name: 'Fontsource',
        url: 'https://fontsource.org/',
        use: 'the font files',
        license: 'Each font under its own license, mostly OFL-1.1',
      },
      { name: 'Lucide', url: 'https://lucide.dev/', use: 'the icons', license: 'ISC' },
      {
        name: 'Phosphor Icons',
        url: 'https://phosphoricons.com/',
        use: 'a second icon set',
        license: 'MIT',
      },
    ],
  },
]
