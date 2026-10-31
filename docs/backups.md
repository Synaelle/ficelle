# Backups

Backups are encrypted copies, made on a schedule, of your notes on a device (Settings, Backups) or of a whole server (admin page, Backups). Only what changed is stored each time.

## Where they go

| Place  | Devices | Servers | For                                        |
| ------ | ------- | ------- | ------------------------------------------ |
| Folder | Yes     | Yes     | another disk, a USB drive, a synced folder |
| S3     | Yes     | Yes     | Backblaze B2, AWS, Wasabi, R2, MinIO       |
| WebDAV | Yes     | Yes     | Nextcloud, ownCloud, most NAS              |
| SFTP   | No      | Yes     | any machine reachable over SSH             |

The short setup takes a folder and a password, with a daily backup. **Advanced** gives the other places, the schedule and how many backups are kept. On a phone, the folder is in Documents and does not survive the phone: copy it elsewhere now and then.

## The password

Backups are encrypted with a password of their own, and restoring needs only that password. Keep it apart from your notes, in a password manager or on paper. Lost, nobody can open the backups. Several devices can share one place and password, each keeping its own backups.

## Restoring

**Show the backups** lists them. **Restore** brings back what is missing, notes deleted since included, and changes nothing that is still there. **Test restore** checks a backup in a scratch copy without touching your notes.

On a new device, make a vault, then in Settings, Backups, choose the same place and **Use a backup already there** with its password. The old device's backups show under its name.

## On a server

A server backup holds the database, every file and the master key, which decrypts standard accounts' notes. A backup and its password together open everything on standard accounts, so keep the password off the server. End-to-end encrypted accounts stay encrypted with their own keys.

**Bring someone back** restores one person's deleted account and spaces from a backup, without touching anyone else.

To restore a whole server on a new machine, keep a copy of the backup place shown under **To restore the whole instance** on the admin page, then:

```bash
FICELLE_BACKUP_PASSWORD='…' ficelle-server backup list --target target.json
FICELLE_BACKUP_PASSWORD='…' ficelle-server backup restore --target target.json --into /var/lib/ficelle-restored
```

Point `storage.data_dir` at the restored folder and start the server. The apps send back anything they hold that the restored server lacks.
