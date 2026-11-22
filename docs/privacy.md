# Privacy

Ficelle has no analytics, tracking or crash reports. Requests carry no account, note or device details, though sites see your network address.

## The apps

- Updates come from GitHub releases, checked on unlock and every few hours. An update downloads only when you ask for it.
- OpenDyslexic downloads once from ficelle.synaelle.com, else jsDelivr.
- Feedback goes to ficelle.synaelle.com, or to your sync server if its admin chose so.

## Feedback

A report holds your text, optional contact and images, the app version, the platform and, unless unticked, recent errors with paths, emails and note titles stripped. The form shows exactly what is sent. A random install id groups reports.

ficelle.synaelle.com deletes reports 30 days after arrival and stores no network address. To see or delete yours sooner, write to synaelle@protonmail.com. You can also complain to the CNIL.

## Sync

Local sync talks only to paired devices on your network, encrypted, and announces a random name there. On Linux open TCP 47821 and UDP 5353.

Server sync talks only to your server. It sees your username, email, device names and sync times, and logs sign-ins with network addresses for 365 days by default. Without end-to-end encryption, whoever runs it can read your notes. With it, the server sees only their number, sizes and change times.

## The web version

Notes stay in the browser, encrypted once you set a password, and never reach the server, which logs no visits and sets no cookies. Cloudflare carries the traffic. Clearing site data deletes the notes. Safari may delete them after a week without a visit unless the site is on the Home Screen.

## On your devices

Notes, files and backups are encrypted once you turn on encryption in Settings, Security. Markdown exports never are.

## Servers

A server checks ficelle.synaelle.com daily for new releases unless its admin turns that off. Mail goes through the admin's mail server. An admin can send a server issue report, with its version and address, from the admin page.
