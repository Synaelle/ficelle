// SPDX-License-Identifier: AGPL-3.0-or-later

/** Injected by vite.config.ts, see tools/version.mjs. */
declare const __APP_VERSION__: string
/** Date of the built commit, YYYY-MM-DD. */
declare const __APP_BUILD_DATE__: string
/** The sync protocol version, `app_protocol::PROTOCOL_VERSION`. */
declare const __APP_PROTOCOL__: number
/** The portable Windows exe, see src-tauri/src/portable.rs. */
declare const __APP_PORTABLE__: boolean
/** Temporary issue reports outside releases. */
declare const __APP_DEV_REPORTS__: boolean

declare module '*.toml' {
  /** Flat "section.key" strings, see vite/plugins.ts. */
  const strings: Record<string, string>
  export default strings
}

declare module 'virtual:icons' {
  /** Inner svg markup by icon role, see vite/plugins.ts. */
  export const lucide: Record<string, string>
  export const phosphor: Record<string, string>
}
