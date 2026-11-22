// SPDX-License-Identifier: AGPL-3.0-or-later
// Encrypting notes that were kept unencrypted, asked from the settings: the
// setup's password step shows over the whole app (App.svelte), then the
// settings hear how it went.

class EncryptFlow {
  open = $state(false)
  private after: ((done: boolean) => void) | null = null
  /** The notes got encrypted, whatever step the person leaves from. */
  private encrypted = false

  start(after: (done: boolean) => void) {
    this.after = after
    this.encrypted = false
    this.open = true
  }

  /** The settings behind show it at once, before the way in is chosen. */
  encryptedNow() {
    this.encrypted = true
    this.after?.(true)
  }

  finish(done: boolean) {
    this.open = false
    this.after?.(done || this.encrypted)
    this.after = null
  }
}

export const encryptFlow = new EncryptFlow()
