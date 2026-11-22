# Self-hosting with Docker

The recommended setup is one container behind an HTTPS reverse proxy.

## Install

Docker runs without sudo, rootless or through the `docker` group (from the next sign-in).

```bash
sudo usermod -aG docker "$USER"
sudo install -d -o "$USER" -g "$USER" /opt/ficelle
cd /opt/ficelle
curl -fsSLO https://raw.githubusercontent.com/Synaelle/ficelle/main/deploy/compose.yml
curl -fsSL -o .env https://raw.githubusercontent.com/Synaelle/ficelle/main/deploy/.env.example
```

Set `PUBLIC_URL`, `PORT` and the two `PRIVACY_` lines in `.env`.

```bash
docker compose up -d
curl http://127.0.0.1:8080/health
```

Put it behind a reverse proxy from [`deploy/`](../deploy). Then open the one-time setup link from the log to create the admin.

```bash
docker compose logs server | grep setup
```

## Updates

```bash
curl -fsSLO https://raw.githubusercontent.com/Synaelle/ficelle/main/deploy/update/install-server-update
sudo bash install-server-update /opt/ficelle
```

This installs a nightly timer that pulls newer images in the same series, checks their signature with cosign, copies the data first and rolls back a broken version. A new series (0.3 to 0.4) means editing the tag in `compose.yml`.

## Backups

Set them up on the admin page. `master.key` in the volume decrypts your notes unless your account uses end-to-end encryption, so guard any copy of the volume.

## Locked out

```bash
docker compose exec server ficelle-server admin reset YOUR_NAME
```

## Configuration

Settings are `FICELLE_SECTION__KEY` variables in `compose.yml`, with `_FILE` for secrets. [`config.rs`](../apps/server/src/config.rs) lists every key.
