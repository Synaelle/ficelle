// SPDX-License-Identifier: AGPL-3.0-or-later

import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { svelte } from '@sveltejs/vite-plugin-svelte'
import { defineConfig } from 'vitest/config'
import { appVersion, buildDate, hasReleaseIdentity, isRelease } from '../../tools/version.mjs'
import { iconSets, tomlLocales } from './vite/plugins.ts'

// tauri sets TAURI_ENV_PLATFORM while it drives the build
const platform = process.env.TAURI_ENV_PLATFORM

/** The sync protocol this app speaks, read from the crate that defines it. */
function protocolVersion(): number {
  const source = readFileSync(
    fileURLToPath(new URL('../../crates/protocol/src/lib.rs', import.meta.url)),
    'utf8',
  )
  const found = /pub const PROTOCOL_VERSION: u32 = (\d+);/.exec(source)
  if (!found) throw new Error('PROTOCOL_VERSION not found in crates/protocol/src/lib.rs')
  return Number(found[1])
}

export default defineConfig({
  plugins: [svelte(), tomlLocales(), iconSets()],
  define: {
    __APP_VERSION__: JSON.stringify(appVersion()),
    __APP_BUILD_DATE__: JSON.stringify(buildDate()),
    __APP_PROTOCOL__: JSON.stringify(protocolVersion()),
    __APP_RELEASE__: JSON.stringify(hasReleaseIdentity()),
    // the portable Windows exe, see src-tauri/src/portable.rs
    __APP_PORTABLE__: JSON.stringify(!!process.env.APP_PORTABLE),
    // temporary issue reports outside releases
    __APP_DEV_REPORTS__: JSON.stringify(!isRelease()),
  },
  clearScreen: false,
  server: {
    // tauri expects a fixed port, failing beats silently picking another one
    port: 1420,
    strictPort: true,
    // translations and the brand (packages/) live outside the app folder
    fs: { allow: ['../..'] },
    // the admin page talks to a local server during development
    proxy: { '/api': 'http://127.0.0.1:8080' },
  },
  envPrefix: ['VITE_', 'TAURI_ENV_'],
  test: {
    environment: 'happy-dom',
    // the font tests read these as text (`?raw`), other css stays empty
    css: { include: [/styles\/fonts(-themes)?\.css/] },
  },
  // relative asset addresses: the server serves the admin page under any
  // path (server.base_path), the page's <base> says which
  base: './',
  build: {
    // match each platform webview so no extra transpilation ships
    target: platform === 'windows' ? 'chrome120' : platform ? 'safari16' : 'es2022',
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
    rollupOptions: {
      // the admin page ships with the web build, the server serves it at
      // /admin; the apps leave it out
      input: {
        main: fileURLToPath(new URL('index.html', import.meta.url)),
        ...(platform ? {} : { admin: fileURLToPath(new URL('admin.html', import.meta.url)) }),
      },
    },
  },
})
