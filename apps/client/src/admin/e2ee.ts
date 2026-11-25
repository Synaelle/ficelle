// SPDX-License-Identifier: AGPL-3.0-or-later
// End-to-end encrypted accounts on the web pages. The OPAQUE math and the
// account keys run in WebAssembly built from the same Rust crate as the
// apps (crates/e2ee-web). It loads on first use, a standard sign-in never
// fetches it. The password is only ever handed to that module, and the
// export key of the page's sign-in stays in it, for the backup key.

import { api, type AccountKind, type KeyBundle, type Session } from './api.svelte'

type Module = typeof import('../generated/e2ee/app_e2ee_web.js')

let loading: Promise<Module> | null = null

function client(): Promise<Module> {
  loading ??= import('../generated/e2ee/app_e2ee_web.js').then(async (module) => {
    await module.default()
    return module
  })
  return loading
}

/** Which way to sign in as `username`. */
export async function kindOf(username: string): Promise<AccountKind> {
  return (await api<{ kind: AccountKind }>('POST', '/auth/kind', { username })).kind
}

/** An app signing in through this page: its one-time public key and the
 * sign-in's challenge, the export key is sealed to them. */
export interface AppKey {
  key: string
  challenge: string
}

/** Runs an OPAQUE sign-in through `start` and `finish` endpoints: the
 * finish answer, and for an app the export key sealed to it. */
async function exchange<T>(
  password: string,
  username: string,
  start: string,
  finish: string,
  extra: Record<string, unknown>,
  forApp?: AppKey,
): Promise<{ answer: T; sealed?: string }> {
  const wasm = await client()
  const first = JSON.parse(wasm.signInStart(password)) as { state: string; request: string }
  const started = await api<{ challenge: string; response: string; ksf?: number }>('POST', start, {
    username,
    request: first.request,
  })
  let finalization: string
  let sealed: string | undefined
  try {
    // how the account's record stretches the password, 1 from an older server
    const ksf = started.ksf ?? 1
    const done = JSON.parse(
      forApp
        ? wasm.signInFinishForApp(
            first.state,
            password,
            started.response,
            username,
            ksf,
            forApp.key,
            forApp.challenge,
          )
        : wasm.signInFinish(first.state, password, started.response, username, ksf),
    ) as { finalization: string; sealed?: string }
    finalization = done.finalization
    sealed = done.sealed
  } catch {
    // a wrong password fails here already: the server still gets an answer
    // so it counts the failure, and replies like for any wrong password
    finalization = 'AA'
  }
  const answer = await api<T>('POST', finish, {
    challenge: started.challenge,
    finalization,
    ...extra,
  })
  return { answer, sealed }
}

/** Signs in to an e2ee account. Answers like a password sign-in: a session,
 * or the second factor challenge. For an app, also its sealed export key. */
export function signIn(username: string, password: string, device: unknown, forApp?: AppKey) {
  return exchange<unknown>(
    password,
    username,
    '/auth/opaque/login/start',
    '/auth/opaque/login/finish',
    { device },
    forApp,
  )
}

/** A proof of the signed in account's password, for a sensitive change. */
export async function proofOf(username: string, password: string): Promise<string> {
  const { answer } = await exchange<{ proof: string }>(
    password,
    username,
    '/me/confirm/start',
    '/me/confirm/finish',
    {},
  )
  return answer.proof
}

/** What an e2ee account answers the second step with when asked for a
 * recovery code: the verifier of its recovery code, which itself opens the
 * keys and stays here, or a second step code from before as typed. */
export async function secondStepCode(typed: string): Promise<string> {
  const wasm = await client()
  try {
    return wasm.recoveryVerifier(typed)
  } catch {
    // not the shape of an encryption recovery code
    return typed
  }
}

/** What setup sends to make the first admin an e2ee account. */
export interface SetupE2ee {
  upload: string
  ksf: number
  keys: KeyBundle
  recoveryVerifier: string
}

/** Registers the first admin's password over OPAQUE during setup and makes
 * its keys. The recovery code is for the page to show once, it is the only
 * way back in if the password is forgotten. */
export async function setupAccount(
  session: string,
  username: string,
  password: string,
): Promise<{ e2ee: SetupE2ee; recoveryCode: string }> {
  const wasm = await client()
  const first = JSON.parse(wasm.signUpStart(password)) as { state: string; request: string }
  const { response } = await api<{ response: string }>('POST', '/setup/registration', {
    session,
    username,
    request: first.request,
  })
  const made = JSON.parse(wasm.signUpFinish(first.state, password, response, username)) as {
    recoveryCode: string
  } & SetupE2ee
  const { recoveryCode, ...e2ee } = made
  return { e2ee, recoveryCode }
}

/** Sets a new password for an e2ee account with its recovery code. The
 * code opens the keys inside the WebAssembly module, the password and the
 * keys never reach the server in the clear. Every device is signed out,
 * this one is signed in. */
export async function recover(
  username: string,
  code: string,
  password: string,
  device: unknown,
): Promise<Session> {
  const wasm = await client()
  const verifier = wasm.recoveryVerifier(code)
  const started = await api<{ challenge: string; keys: KeyBundle }>(
    'POST',
    '/auth/recovery/start',
    { username, verifier },
  )
  const first = JSON.parse(wasm.signUpStart(password)) as { state: string; request: string }
  const { response } = await api<{ response: string }>('POST', '/auth/opaque/registration', {
    username,
    request: first.request,
  })
  const done = JSON.parse(
    wasm.recoverFinish(
      code,
      JSON.stringify(started.keys),
      first.state,
      password,
      response,
      username,
    ),
  ) as { upload: string; ksf: number; keys: KeyBundle }
  return api<Session>('POST', '/auth/recovery/finish', {
    challenge: started.challenge,
    upload: done.upload,
    ksf: done.ksf,
    keys: done.keys,
    device,
  })
}

/** Locks the server's backups when they wait for it: a new backup key,
 * wrapped by the signed in account's master key, which the export key of
 * this page's sign-in opens inside the module. Whether it did. Asked twice
 * at once, the second waits for the first. */
export function lockBackups(): Promise<boolean> {
  locking ??= lock().finally(() => (locking = null))
  return locking
}

let locking: Promise<boolean> | null = null

async function lock(): Promise<boolean> {
  const wasm = await client()
  if (!wasm.holdsKey()) return false
  const view = await api<{ keyWanted: boolean }>('GET', '/admin/backups')
  if (!view.keyWanted) return false
  const { keys } = await api<{ keys: KeyBundle }>('GET', '/me/keys')
  const made = JSON.parse(wasm.backupKey(JSON.stringify(keys))) as { key: string; wrapped: string }
  await api('PUT', '/admin/backups/key', made)
  return true
}

/** What a fresh server finds beside backups locked by a backup key. */
export interface KeyFile {
  username: string
  keys: KeyBundle
  backupKey: string
}

/** The backup key, opened with the account's recovery code. The code
 * stays here, the key goes to the server that restores. */
export async function openBackupKey(code: string, file: KeyFile): Promise<string> {
  const wasm = await client()
  return wasm.openBackupKey(code, JSON.stringify(file.keys), file.backupKey)
}
