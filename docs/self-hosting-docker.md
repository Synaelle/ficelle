# Self-hosting with Docker

The server is one container, `ghcr.io/synaelle/ficelle-server`, with its data in one volume. Put a reverse proxy with HTTPS in front of it. This is the recommended way to run a server.

## Install

Docker should run without sudo for the account that runs the server. If `docker ps` asks for it, add that account to the `docker` group once, then sign out and back in. Membership in this group is as strong as root on the machine, so keep it to whoever runs the server. Rootless Docker works too.

```bash
sudo usermod -aG docker "$USER"
```

Give the server's folder to that account once. Nothing after this needs sudo, apart from installing the update timer.

```bash
sudo install -d -o "$USER" -g "$USER" /opt/ficelle
cd /opt/ficelle
curl -fsSLO https://raw.githubusercontent.com/Synaelle/ficelle/main/deploy/compose.yml
curl -fsSL -o .env https://raw.githubusercontent.com/Synaelle/ficelle/main/deploy/.env.example
```

In `.env`, set `PUBLIC_URL` (the HTTPS address people use), `PORT` (on `127.0.0.1`, 8080 by default) and the two `PRIVACY_` lines shown on the server's privacy page. Then:

```bash
docker compose up -d
curl http://127.0.0.1:8080/health   # ok
```

`compose.yml` follows the current release series, publishes on `127.0.0.1` only and runs unprivileged on a read-only system. The server makes its own secrets on first start, inside the volume. Under a path, the health check is under it too, as in `curl http://127.0.0.1:8080/notes/health`.

## Reverse proxy

Ready-made configs are in [`deploy/`](../deploy): nginx on a subdomain or under a path, Caddy, and Traefik with Let's Encrypt. The proxy must pass `X-Forwarded-For`, `-Proto` and `-Host`, allow WebSocket upgrades, accept 16 MB bodies and keep connections open for an hour.

## First admin

Until an admin exists, the server logs a one-time setup link, valid for an hour:

```bash
docker compose logs server | grep setup
```

The setup checks the proxy, creates the admin with a passkey or an authenticator app, and offers backups. Then sign in from the apps with the server's address.

## Updates

The timer runs as a system service, so installing it is the one step that needs sudo.

```bash
curl -fsSLO https://raw.githubusercontent.com/Synaelle/ficelle/main/deploy/update/install-server-update
sudo bash install-server-update /opt/ficelle
```

This installs a timer that runs each night, pulls the newer image of the series the tag in `compose.yml` names, copies the data first and rolls back if the new version is not healthy. It checks the image's signature with cosign, and does not update until cosign is installed, unless `ALLOW_UNSIGNED=1` is set for the service. A new series (0.3 to 0.4) means changing the tag in `compose.yml` after reading the release notes. The image is also tagged `latest`, which crosses series, and `nightly`, the code of main each night.

## Backups

Set them up on the admin page, under Backups: S3, WebDAV, SFTP or a folder, encrypted with their own password. Keep that password off the server. `master.key` in the volume decrypts standard accounts' notes, so a copy of the volume is as sensitive as the server.

## Locked out

```bash
docker compose exec server ficelle-server admin reset YOUR_NAME
```

It prints a new password, removes the second steps, lifts the sign-in lock and signs out every device. An end-to-end encrypted admin cannot be reset, only its recovery code opens it.

## Configuration

Settings live in environment variables in `compose.yml`, `FICELLE_SECTION__KEY`, with `_FILE` at the end to read a value from a file. Passwords and secrets are refused as plain variables, give them with `_FILE`. Every key and its default is described in [`apps/server/src/config.rs`](../apps/server/src/config.rs).
