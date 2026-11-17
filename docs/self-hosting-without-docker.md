# Self-hosting without Docker

The server is one binary, `ficelle-server`, with the web pages beside it and its data in one folder. It runs as a systemd service behind a reverse proxy with HTTPS.

The commands below are written for Debian and Ubuntu. Other Linux systems with systemd work the same way, with their own package names and paths.

> [!WARNING]
> Running without Docker is supported but not recommended. You build from source, update by hand and get no automatic rollback or image signature check. Use [self-hosting with Docker](self-hosting-docker.md) unless you know why you need this.

## Build

The server is built from source.

The build needs these, named as Debian and Ubuntu packages, which may differ on other distributions.

| Need         | How                                                                                                        |
| ------------ | ---------------------------------------------------------------------------------------------------------- |
| Build tools  | `sudo apt install git curl build-essential pkg-config perl clang`                                          |
| Rust         | [rustup](https://rustup.rs), which reads the version and the WebAssembly target from `rust-toolchain.toml` |
| Node.js 24   | from [nodejs.org](https://nodejs.org) or NodeSource, the distribution's own is often older                 |
| pnpm         | `npm install -g pnpm@12.15.0`, the version in `package.json`                                               |
| wasm-bindgen | `cargo install --locked wasm-bindgen-cli --version 0.2.129`, the version in `Cargo.lock`                   |

Perl and the build tools are for OpenSSL, built from source for the encrypted database. Clang builds the web version.

Build the latest server release, not `main`:

```bash
git clone https://github.com/Synaelle/ficelle.git && cd ficelle
git checkout "$(git describe --tags --abbrev=0 --match 'server-v*')"
pnpm install
APP_RELEASE=1 pnpm --filter @app/client build
APP_RELEASE=1 pnpm --filter @app/client build:web
APP_RELEASE=1 cargo build --release -p server
```

Without `APP_RELEASE=1` the server calls itself Ficelle Dev and reports a development version.

## Install

```bash
sudo useradd --system --home /var/lib/ficelle --shell /usr/sbin/nologin ficelle
sudo install -m 755 target/release/ficelle-server /usr/local/bin/
sudo mkdir -p /usr/local/share/ficelle /etc/ficelle
sudo cp -r apps/client/dist /usr/local/share/ficelle/web
sudo cp -r apps/client/dist-web /usr/local/share/ficelle/web/web
sudo ficelle-server --config /etc/ficelle/ficelle.toml setup
sudo chown ficelle: /etc/ficelle/ficelle.toml
sudo cp deploy/systemd/ficelle-server.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now ficelle-server
curl http://127.0.0.1:8080/health   # ok
```

`setup` asks for the public address, the address and port to listen on (keep 127.0.0.1:8080), the data folder and, if you want, a mail server. Answer `/var/lib/ficelle` for the data folder. It writes the config readable by its owner only. The service runs as `ficelle`, may write only to `/var/lib/ficelle`, and the server makes its own secrets there on first start.

Under a path, the health check is under it too, as in `curl http://127.0.0.1:8080/notes/health`.

## Reverse proxy

Ready-made configs are in [`deploy/`](../deploy): nginx on a subdomain or under a path, and Caddy. The proxy must pass `X-Forwarded-For`, `-Proto` and `-Host`, allow WebSocket upgrades, accept 16 MB bodies and keep connections open for an hour.

## First admin

Until an admin exists, the server logs a one-time setup link, valid for an hour:

```bash
sudo journalctl -u ficelle-server | grep setup
```

The setup checks the proxy, creates the admin with a passkey or an authenticator app, and offers backups. Then sign in from the apps with the server's address.

## Updates

Fetch the newest server release, rebuild, then copy the binary and the web pages again and restart:

```bash
git fetch --tags && git checkout "$(git describe --tags --abbrev=0 --match 'server-v*' origin/main)"
pnpm install && APP_RELEASE=1 pnpm --filter @app/client build && APP_RELEASE=1 pnpm --filter @app/client build:web
APP_RELEASE=1 cargo build --release -p server
sudo systemctl stop ficelle-server
sudo cp -a /var/lib/ficelle /var/lib/ficelle.before-update
sudo install -m 755 target/release/ficelle-server /usr/local/bin/
sudo rm -rf /usr/local/share/ficelle/web && sudo cp -r apps/client/dist /usr/local/share/ficelle/web
sudo cp -r apps/client/dist-web /usr/local/share/ficelle/web/web
sudo systemctl start ficelle-server
```

The server updates its database on start. Keep the copy until the new version works.

## Backups

Set them up on the admin page, under Backups: S3, WebDAV, SFTP or a folder, encrypted with their own password. Keep that password off the server. A folder elsewhere on the machine needs a `ReadWritePaths=` line (`sudo systemctl edit ficelle-server`). `master.key` in `/var/lib/ficelle` decrypts standard accounts' notes, so a copy of that folder is as sensitive as the server.

## Locked out

```bash
sudo -u ficelle ficelle-server --config /etc/ficelle/ficelle.toml admin reset YOUR_NAME
```

It prints a new password, removes the second steps, lifts the sign-in lock and signs out every device. An end-to-end encrypted admin cannot be reset, only its recovery code opens it.

## Configuration

Settings live in `/etc/ficelle/ficelle.toml` or in environment variables, `FICELLE_SECTION__KEY`, with `_FILE` at the end to read a value from a file. Passwords and secrets are refused as plain variables, give them with `_FILE`. Every key and its default is described in [`apps/server/src/config.rs`](../apps/server/src/config.rs).
