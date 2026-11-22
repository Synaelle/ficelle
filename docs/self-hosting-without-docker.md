# Self-hosting without Docker

> [!WARNING]
> Not recommended and not supported. You build and update by hand, and I only test the [Docker image](self-hosting-docker.md).

Commands are for Debian and Ubuntu.

## Build

You need [rustup](https://rustup.rs), Node.js 24 and these packages. pnpm comes through corepack, and `wasm-bindgen-cli` must match the `wasm-bindgen` version in `Cargo.lock`.

```bash
sudo apt install git curl build-essential pkg-config perl clang
git clone https://github.com/Synaelle/ficelle.git && cd ficelle
git checkout "$(git describe --tags --abbrev=0 --match 'server-v*')"
cargo install --locked wasm-bindgen-cli --version "$(grep -A1 '^name = "wasm-bindgen"$' Cargo.lock | sed -n 's/version = "\(.*\)"/\1/p')"
sudo corepack enable && pnpm install --frozen-lockfile
APP_RELEASE=1 pnpm --filter @app/client build:admin
APP_RELEASE=1 pnpm --filter @app/client build:web
APP_RELEASE=1 cargo build --release --locked -p server
```

## Install

```bash
sudo useradd --system --home /var/lib/ficelle --shell /usr/sbin/nologin ficelle
sudo install -m 755 target/release/ficelle-server /usr/local/bin/
sudo mkdir -p /usr/local/share/ficelle /etc/ficelle
sudo cp -r apps/client/dist /usr/local/share/ficelle/web
sudo cp -r apps/client/dist-web /usr/local/share/ficelle/web/web
sudo node tools/fonts-mirror.mjs /usr/local/share/ficelle/web/fonts
sudo ficelle-server --config /etc/ficelle/ficelle.toml setup
sudo chown ficelle: /etc/ficelle/ficelle.toml
sudo cp deploy/systemd/ficelle-server.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now ficelle-server
curl http://127.0.0.1:8080/health
```

Put it behind a reverse proxy from [`deploy/`](../deploy). Then open the one-time setup link from the log to create the admin.

```bash
sudo journalctl -u ficelle-server | grep setup
```

## Updates

```bash
git fetch --tags && git checkout "$(git describe --tags --abbrev=0 --match 'server-v*' origin/main)"
pnpm install --frozen-lockfile && APP_RELEASE=1 pnpm --filter @app/client build:admin && APP_RELEASE=1 pnpm --filter @app/client build:web
APP_RELEASE=1 cargo build --release --locked -p server
sudo systemctl stop ficelle-server
sudo cp -a /var/lib/ficelle /var/lib/ficelle.before-update
sudo install -m 755 target/release/ficelle-server /usr/local/bin/
sudo rm -rf /usr/local/share/ficelle/web && sudo cp -r apps/client/dist /usr/local/share/ficelle/web
sudo cp -r apps/client/dist-web /usr/local/share/ficelle/web/web
sudo node tools/fonts-mirror.mjs /usr/local/share/ficelle/web/fonts
sudo systemctl start ficelle-server
```

Keep the copy until the new version works.

## Backups

Set them up on the admin page. A folder outside `/var/lib/ficelle` needs a `ReadWritePaths=` line (`sudo systemctl edit ficelle-server`). `master.key` decrypts standard accounts' notes, so guard any copy of it.

## Locked out

```bash
sudo -u ficelle ficelle-server --config /etc/ficelle/ficelle.toml admin reset YOUR_NAME
```

## Configuration

Settings live in `/etc/ficelle/ficelle.toml` or `FICELLE_SECTION__KEY` variables, with `_FILE` for secrets. [`config.rs`](../apps/server/src/config.rs) lists every key.
