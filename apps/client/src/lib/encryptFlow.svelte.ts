// SPDX-License-Identifier: AGPL-3.0-or-later
// Encrypting notes that were kept unencrypted, asked from the settings: the
// setup's password step shows over the whole app (App.svelte), then the
// settings hear how it went.

class EncryptFlow {
  open = $state(false)
  private after: ((done: boolean) => void) | null = null

  start(after: (done: boolean) => void) {
    this.after = after
    this.open = true
  }

  finish(done: boolean) {
    this.open = false
    this.after?.(done)
    this.after = null
  }
}

export const encryptFlow = new EncryptFlow()
