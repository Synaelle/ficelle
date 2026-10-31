// SPDX-License-Identifier: AGPL-3.0-or-later
// End-to-end encrypted accounts on the web pages. The OPAQUE math and the
// account keys run in WebAssembly built from the same Rust crate as the
// apps (crates/e2ee-web). It loads on first use, a standard sign-in never
// fetches it. The password is only ever handed to that module.

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

/** Runs an OPAQUE sign-in through `start` and `finish` endpoints: the
 * finish answer. */
async function exchange<T>(
  password: string,
  username: string,
  start: string,
  finish: string,
  extra: Record<string, unknown>,
): Promise<T> {
  const wasm = await client()
  const first = JSON.parse(wasm.signInStart(password)) as { state: string; request: string }
  const started = await api<{ challenge: string; response: string; ksf?: number }>('POST', start, {
    username,
    request: first.request,
  })
  let finalization: string
  try {
    // how the account's record stretches the password, 1 from an older server
    const ksf = started.ksf ?? 1
    finalization = (
      JSON.parse(wasm.signInFinish(first.state, password, started.response, username, ksf)) as {
        finalization: string
      }
    ).finalization
  } catch {
    // a wrong password fails here already: the server still gets an answer
    // so it counts the failure, and replies like for any wrong password
    finalization = 'AA'
  }
  return api<T>('POST', finish, { challenge: started.challenge, finalization, ...extra })
}

/** Signs in to an e2ee account. Answers like a password sign-in: a session,
 * or the second factor challenge. */
export function signIn(username: string, password: string, device: unknown) {
  return exchange<unknown>(
    password,
    username,
    '/auth/opaque/login/start',
    '/auth/opaque/login/finish',
    { device },
  )
}

/** A proof of the signed in account's password, for a sensitive change. */
export async function proofOf(username: string, password: string): Promise<string> {
  const answer = await exchange<{ proof: string }>(
    password,
    username,
    '/me/confirm/start',
    '/me/confirm/finish',
    {},
  )
  return answer.proof
}

/** Creates an e2ee account. The recovery code in the answer is shown once,
 * it is the only way back in if the password is forgotten. */
export async function signUp(
  username: string,
  password: string,
  rest: { displayName?: string; invite?: string | null; device: unknown },
): Promise<{ session: Session; recoveryCode: string }> {
  const wasm = await client()
  const first = JSON.parse(wasm.signUpStart(password)) as { state: string; request: string }
  const { response } = await api<{ response: string }>('POST', '/auth/opaque/registration', {
    username,
    request: first.request,
  })
  const made = JSON.parse(wasm.signUpFinish(first.state, password, response, username)) as {
    upload: string
    ksf: number
    keys: KeyBundle
    recoveryVerifier: string
    recoveryCode: string
  }
  const session = await api<Session>('POST', '/auth/e2ee/register', {
    username,
    upload: made.upload,
    ksf: made.ksf,
    keys: made.keys,
    recoveryVerifier: made.recoveryVerifier,
    ...rest,
  })
  return { session, recoveryCode: made.recoveryCode }
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
