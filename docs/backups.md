# Backups

## On a device

Turn on Back up automatically in Settings, Data and backups, and choose a folder. Each backup is one `.ficelle` file, and the latest 10 stay. On a phone the folder dies with the phone, so copy it elsewhere now and then.

Backups of encrypted notes need the notes password they were made with. Backups of unencrypted notes have no password.

Restore brings back what is missing and changes nothing still there.

## On a server

Set up backups to a folder on the admin page. The server backs up once a day and keeps as many backups as you choose. With a standard account, a backup opens your notes with its password, so keep that password off the server. With an end-to-end encrypted account, your devices already seal your notes, and the backups have no password. To restore a whole server, stop it and restore into an empty folder. Leave out `FICELLE_BACKUP_PASSWORD` for backups without a password.

```bash
FICELLE_BACKUP_PASSWORD='…' ficelle-server backup restore --target target.json --into /var/lib/ficelle
sudo chown -R ficelle: /var/lib/ficelle
```

With Docker, restore into an empty volume.

```bash
docker run --rm -e FICELLE_BACKUP_PASSWORD -v "$PWD/target.json:/target.json:ro" -v ficelle_backups:/backups:ro -v ficelle_data:/data ghcr.io/synaelle/ficelle-server:latest backup restore --target /target.json --into /data
```

`target.json` is the backup place shown on the admin page under To restore the whole instance.
