# Self-hosting with Docker

The server is one container, `ghcr.io/synaelle/ficelle-server`, with its data in one volume. Put a reverse proxy with HTTPS in front of it. This is the recommended and supported way to run a server.

## Install

```bash
mkdir -p /opt/ficelle && cd /opt/ficelle
curl -fsSLO https://raw.githubusercontent.com/Synaelle/ficelle/main/deploy/compose.yml
curl -fsSL -o .env https://raw.githubusercontent.com/Synaelle/ficelle/main/deploy/.env.example
```

In `.env`, set `PUBLIC_URL` (the HTTPS address people use), `PORT` (on `127.0.0.1`, 8080 by default) and the two `PRIVACY_` lines shown on the server's privacy page. Then:

```bash
docker compose up -d
curl http://127.0.0.1:8080/health   # ok
```

`compose.yml` follows the current release series, publishes on `127.0.0.1` only and runs unprivileged on a read-only system. The server makes its own secrets on first start, inside the volume.

## Reverse proxy

Ready-made configs are in [`deploy/`](../deploy): nginx on a subdomain or under a path, Caddy, and Traefik with Let's Encrypt. The proxy must pass `X-Forwarded-For`, `-Proto` and `-Host`, allow WebSocket upgrades, accept 16 MB bodies and keep connections open for an hour.

## First admin

Until an admin exists, the server logs a one-time setup link, valid for an hour:

```bash
docker compose logs server | grep setup
```

The setup checks the proxy, asks whether the server is for you alone or for several people, creates the admin with a passkey or an authenticator app, and offers backups. Then sign in from the apps with the server's address.

## Updates

```bash
curl -fsSLO https://raw.githubusercontent.com/Synaelle/ficelle/main/deploy/update/install-server-update
sudo bash install-server-update /opt/ficelle
```

This installs a nightly timer that pulls the newer image of the series, copies the data first and rolls back if the new version is not healthy. With cosign installed, it also checks the image's signature. A new series (0.3 to 0.4) means changing the tag in `compose.yml` after reading the release notes.

## Backups

Set them up on the admin page, under Backups: S3, WebDAV, SFTP or a folder, encrypted with their own password. Keep that password off the server. `master.key` in the volume decrypts standard accounts' notes, so a copy of the volume is as sensitive as the server.

## Locked out

```bash
docker compose exec server ficelle-server admin reset YOUR_NAME
```

It prints a new password, removes the second steps and signs out every device.

## Configuration

Settings live in `ficelle.toml` or in environment variables, `FICELLE_SECTION__KEY`, with `_FILE` at the end to read a value from a file. Every key and its default is described in [`apps/server/src/config.rs`](../apps/server/src/config.rs).
