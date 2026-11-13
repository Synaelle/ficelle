// SPDX-License-Identifier: AGPL-3.0-or-later

/** Injected by vite.config.ts, see tools/version.mjs. */
declare const __APP_VERSION__: string
/** Date of the built commit, YYYY-MM-DD. */
declare const __APP_BUILD_DATE__: string
/** The sync protocol version, `app_protocol::PROTOCOL_VERSION`. */
declare const __APP_PROTOCOL__: number
/** The portable Windows exe, see src-tauri/src/portable.rs. */
declare const __APP_PORTABLE__: boolean
/** Everything only for builds that are not releases: the report tool, test hooks, shorter waits. */
declare const __APP_DEV_TOOLS__: boolean
/** The system the build is for, see lib/platform.ts. */
declare const __APP_PLATFORM__: 'windows' | 'darwin' | 'linux' | 'android' | 'web'
/** The web version (`vite build --mode web`), see lib/platform.ts. */
declare const __APP_WEB__: boolean

declare module '*.toml' {
  /** Flat "section.key" strings, see vite/plugins.ts. */
  const strings: Record<string, string>
  export default strings
}

declare module 'virtual:locale-shares' {
  /** Each language's share of the English texts, see vite/plugins.ts. */
  const shares: Record<string, number>
  export default shares
}

declare module 'virtual:icons' {
  /** Inner svg markup by icon role, see vite/plugins.ts. */
  export const lucide: Record<string, string>
  export const phosphor: Record<string, string>
}
