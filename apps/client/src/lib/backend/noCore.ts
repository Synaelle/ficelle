// SPDX-License-Identifier: AGPL-3.0-or-later
// Stands for the notes core (crates/core-web) in every build but the web
// version's, which alone builds it (tools/wasm.mjs --core): its worker is
// bundled everywhere, and only ever started there.

const missing = () => {
  throw new Error('the notes core is only in the web version')
}

export default async function init() {
  missing()
}
export const start = missing
export const call: (name: string, args: string) => string = missing
export const callBytes: (name: string, args: string, bytes: Uint8Array) => string = missing
export const readBytes: (name: string, args: string) => Uint8Array = missing
