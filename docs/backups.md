# Backups

Backups are copies of your notes on a device (Settings, Data and backups) or of a whole server (admin page, Backups), made on a schedule. Only what changed is stored each time.

## Places

| Place  | Devices           | Servers | For                                        |
| ------ | ----------------- | ------- | ------------------------------------------ |
| Folder | Yes               | Yes     | another disk, a USB drive, a synced folder |
| S3     | If set up earlier | Yes     | Backblaze B2, AWS, Wasabi, R2, MinIO       |
| WebDAV | If set up earlier | Yes     | Nextcloud, ownCloud, most NAS              |
| SFTP   | No                | Yes     | any machine reachable over SSH             |

On a device, turn on Back up automatically and choose a folder. A backup is made right away, then every day while the app is open. Backups set up earlier with S3 or WebDAV, or with their own schedule, keep working as they were. On a phone, the folder is in Documents and does not survive the phone, so copy it elsewhere now and then.

## The password

When your notes are encrypted, so are their backups, with your notes password or one of their own, and restoring needs only that password. Keep it in a password manager or on paper. Lost, nobody can open the backups. When your notes are not encrypted, their backups ask for no password, so keep them somewhere private. Several devices can share one place and password, each keeping its own backups.

## Restoring

Restore from a backup lists them. Restore brings back what is missing, notes deleted since included, and changes nothing that is still there. Restore from a file opens a one-file backup.

On a new device, open Restore from a backup, choose the same folder and enter its password. The newest backup is offered. The old device's backups show under its name.

## On a server

A server backup holds the database, every file and the master key, which decrypts standard accounts' notes. A backup and its password together open everything on standard accounts, so keep the password off the server. End-to-end encrypted accounts stay encrypted with their own keys.

Bring someone back, on the admin page, restores one person's deleted account and spaces from a backup, without touching anyone else.

To restore a whole server on a new machine, keep a copy of the backup place shown under To restore the whole instance on the admin page, then:

```bash
FICELLE_BACKUP_PASSWORD='…' ficelle-server backup list --target target.json
FICELLE_BACKUP_PASSWORD='…' ficelle-server backup restore --target target.json --into /var/lib/ficelle-restored
```

Point `storage.data_dir` at the restored folder and start the server. The apps send back anything they hold that the restored server lacks.
