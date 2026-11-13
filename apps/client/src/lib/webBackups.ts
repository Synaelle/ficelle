// SPDX-License-Identifier: AGPL-3.0-or-later
// The web version keeps the notes in the browser, which can lose them: it
// asks the browser to keep them, and reminds the person to download a
// backup when none was made for two weeks. Dates are kept in this browser.

const LAST_BACKUP = 'lastBackupAt'
const NOTES_SINCE = 'webNotesSince'
const REMIND_AFTER = 14 * 24 * 60 * 60 * 1000

function read(key: string): number {
  try {
    return Number(localStorage.getItem(key) ?? 0)
  } catch {
    return 0
  }
}

function write(key: string) {
  try {
    localStorage.setItem(key, String(Date.now()))
  } catch {
    // a browser that keeps nothing reminds again later
  }
}

/** The notes were just made: the browser is asked to keep them. */
export function notesMade() {
  write(NOTES_SINCE)
  void navigator.storage?.persist?.()
}

export function backupMade() {
  write(LAST_BACKUP)
}

/** Two weeks since the last backup, or since the notes were made. */
export function backupDue(): boolean {
  const last = Math.max(read(LAST_BACKUP), read(NOTES_SINCE))
  return last > 0 && Date.now() - last > REMIND_AFTER
}
