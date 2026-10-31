// SPDX-License-Identifier: AGPL-3.0-or-later
// Passkeys in the browser. The server speaks WebAuthn json, with binary
// fields in base64url. The browser api takes and returns ArrayBuffers, so
// both directions are converted here, by hand: the json helpers of the
// api itself are missing from older browsers.

type Json = Record<string, unknown>

function toBytes(text: string): ArrayBuffer {
  const base64 = text.replace(/-/g, '+').replace(/_/g, '/')
  const binary = atob(base64 + '='.repeat((4 - (base64.length % 4)) % 4))
  const bytes = new Uint8Array(binary.length)
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i)
  return bytes.buffer
}

function toText(buffer: ArrayBuffer | null): string | null {
  if (!buffer) return null
  let binary = ''
  for (const byte of new Uint8Array(buffer)) binary += String.fromCharCode(byte)
  return btoa(binary).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '')
}

function credentials(list: unknown): PublicKeyCredentialDescriptor[] | undefined {
  if (!Array.isArray(list)) return undefined
  return (list as Json[]).map((c) => ({ ...c, id: toBytes(c.id as string) }) as never)
}

/** Whether this browser can use passkeys at all. */
export function passkeysSupported(): boolean {
  return typeof window.PublicKeyCredential === 'function' && !!navigator.credentials
}

/** Asks the authenticator for a new passkey, `options` as the server sent them. */
export async function createPasskey(options: Json): Promise<Json> {
  const key = options.publicKey as Json
  const user = key.user as Json
  const credential = (await navigator.credentials.create({
    publicKey: {
      ...key,
      challenge: toBytes(key.challenge as string),
      user: { ...user, id: toBytes(user.id as string) },
      excludeCredentials: credentials(key.excludeCredentials),
    } as never,
  })) as PublicKeyCredential | null
  if (!credential) throw new Error('no passkey')
  const response = credential.response as AuthenticatorAttestationResponse
  return {
    id: credential.id,
    rawId: toText(credential.rawId),
    type: credential.type,
    // some password managers stand in for the browser without this method
    extensions: credential.getClientExtensionResults?.() ?? {},
    response: {
      attestationObject: toText(response.attestationObject),
      clientDataJSON: toText(response.clientDataJSON),
      transports: response.getTransports?.() ?? [],
    },
  }
}

/** Asks the authenticator to sign in, `options` as the server sent them. */
export async function usePasskey(options: Json): Promise<Json> {
  const key = options.publicKey as Json
  const credential = (await navigator.credentials.get({
    publicKey: {
      ...key,
      challenge: toBytes(key.challenge as string),
      allowCredentials: credentials(key.allowCredentials),
    } as never,
  })) as PublicKeyCredential | null
  if (!credential) throw new Error('no passkey')
  const response = credential.response as AuthenticatorAssertionResponse
  return {
    id: credential.id,
    rawId: toText(credential.rawId),
    type: credential.type,
    // some password managers stand in for the browser without this method
    extensions: credential.getClientExtensionResults?.() ?? {},
    response: {
      authenticatorData: toText(response.authenticatorData),
      clientDataJSON: toText(response.clientDataJSON),
      signature: toText(response.signature),
      userHandle: toText(response.userHandle),
    },
  }
}
