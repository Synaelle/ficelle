# Backups

## On a device

Turn on Back up automatically in Settings, Data and backups, and choose a folder. Each backup is one `.ficelle` file, and the latest 10 stay. On a phone the folder dies with the phone, so copy it elsewhere now and then.

Backups of encrypted notes need the notes password they were made with. Backups of unencrypted notes have no password.

Restore brings back what is missing and changes nothing still there.

## On a server

Set up backups on the admin page. With its password, a backup opens your notes unless your account uses end-to-end encryption, so keep that password off the server. To restore a whole server, stop it and restore into an empty folder.

```bash
FICELLE_BACKUP_PASSWORD='…' ficelle-server backup restore --target target.json --into /var/lib/ficelle
sudo chown -R ficelle: /var/lib/ficelle
```

With Docker, restore into an empty volume.

```bash
docker run --rm -e FICELLE_BACKUP_PASSWORD -v "$PWD/target.json:/target.json:ro" -v ficelle_data:/data ghcr.io/synaelle/ficelle-server:0.6 backup restore --target /target.json --into /data
```

`target.json` is the backup place shown on the admin page under To restore the whole instance.
