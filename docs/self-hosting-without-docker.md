# Self-hosting without Docker

The server is one binary, `ficelle-server`, with the web pages beside it and its data in one folder. It runs as a systemd service behind a reverse proxy with HTTPS.

> [!WARNING]
> Running without Docker is not supported and not recommended. You build from source, update by hand and get no automatic rollback or image signature check. Use [self-hosting with Docker](self-hosting-docker.md) unless you know why you need this.

## Build

Needs Rust (rustup), Node.js 24 with pnpm, and `wasm-bindgen-cli` at the version in `Cargo.lock`.

```bash
git clone https://github.com/Synaelle/ficelle.git && cd ficelle
pnpm install
pnpm --filter @app/client build
cargo build --release -p server
```

## Install

```bash
sudo useradd --system --home /var/lib/ficelle --shell /usr/sbin/nologin ficelle
sudo install -m 755 target/release/ficelle-server /usr/local/bin/
sudo mkdir -p /usr/local/share/ficelle /etc/ficelle
sudo cp -r apps/client/dist /usr/local/share/ficelle/web
sudo ficelle-server --config /etc/ficelle/ficelle.toml setup
sudo chown ficelle: /etc/ficelle/ficelle.toml
sudo cp deploy/systemd/ficelle-server.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now ficelle-server
curl http://127.0.0.1:8080/health   # ok
```

`setup` asks for the public address and the port, and writes the config readable by its owner only. The service runs as `ficelle`, may write only to `/var/lib/ficelle`, and the server makes its own secrets there on first start.

## Reverse proxy

Ready-made configs are in [`deploy/`](../deploy): nginx on a subdomain or under a path, and Caddy. The proxy must pass `X-Forwarded-For`, `-Proto` and `-Host`, allow WebSocket upgrades, accept 16 MB bodies and keep connections open for an hour.

## First admin

Until an admin exists, the server logs a one-time setup link, valid for an hour:

```bash
sudo journalctl -u ficelle-server | grep setup
```

The setup checks the proxy, asks whether the server is for you alone or for several people, creates the admin with a passkey or an authenticator app, and offers backups. Then sign in from the apps with the server's address.

## Updates

Pull, rebuild, then copy the binary and the web pages again and restart:

```bash
git pull
pnpm install && pnpm --filter @app/client build && cargo build --release -p server
sudo systemctl stop ficelle-server
sudo cp -a /var/lib/ficelle /var/lib/ficelle.before-update
sudo install -m 755 target/release/ficelle-server /usr/local/bin/
sudo rm -rf /usr/local/share/ficelle/web && sudo cp -r apps/client/dist /usr/local/share/ficelle/web
sudo systemctl start ficelle-server
```

The server updates its database on start. Keep the copy until the new version works.

## Backups

Set them up on the admin page, under Backups: S3, WebDAV, SFTP or a folder, encrypted with their own password. Keep that password off the server. A folder elsewhere on the machine needs a `ReadWritePaths=` line (`sudo systemctl edit ficelle-server`). `master.key` in `/var/lib/ficelle` decrypts standard accounts' notes, so a copy of that folder is as sensitive as the server.

## Locked out

```bash
sudo -u ficelle ficelle-server --config /etc/ficelle/ficelle.toml admin reset YOUR_NAME
```

It prints a new password, removes the second steps and signs out every device.

## Configuration

Settings live in `/etc/ficelle/ficelle.toml` or in environment variables, `FICELLE_SECTION__KEY`, with `_FILE` at the end to read a value from a file. Every key and its default is described in [`apps/server/src/config.rs`](../apps/server/src/config.rs).
