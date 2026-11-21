// SPDX-License-Identifier: AGPL-3.0-or-later

import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { svelte } from '@sveltejs/vite-plugin-svelte'
import { defineConfig } from 'vitest/config'
import { appVersion, buildDate, hasReleaseIdentity, isRelease } from '../../tools/version.mjs'
import { iconSets, localeShares, tomlLocales } from './vite/plugins.ts'

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

// `--mode web` builds the web version: the notes app alone, for a static
// folder, with the core running in the page
export default defineConfig(({ mode }) => ({
  plugins: [svelte(), tomlLocales(!!platform), localeShares(), iconSets()],
  define: {
    __APP_VERSION__: JSON.stringify(appVersion()),
    __APP_BUILD_DATE__: JSON.stringify(buildDate()),
    __APP_PROTOCOL__: JSON.stringify(protocolVersion()),
    __APP_RELEASE__: JSON.stringify(hasReleaseIdentity()),
    // a preview has its own name, id and data, see packages/brand
    __APP_PREVIEW__: JSON.stringify(process.env.APP_PREVIEW === '1'),
    // the portable Windows exe, see src-tauri/src/portable.rs
    __APP_PORTABLE__: JSON.stringify(!!process.env.APP_PORTABLE),
    // the APK for F-Droid, which the store updates
    __APP_STORE__: JSON.stringify(process.env.APP_STORE === '1'),
    // temporary issue reports outside releases
    __APP_DEV_TOOLS__: JSON.stringify(!isRelease()),
    // the system the tauri cli builds for, "web" in the browser and tests
    __APP_PLATFORM__: JSON.stringify(process.env.TAURI_ENV_PLATFORM ?? 'web'),
    // the web version, not the browser preview of development and the tests
    __APP_WEB__: JSON.stringify(mode === 'web'),
  },
  resolve: {
    // components mount in the tests (FileWindow.test.ts): svelte's browser build
    ...(process.env.VITEST ? { conditions: ['browser'] } : {}),
    alias: {
      // the notes core, built for the web version alone (tools/wasm.mjs
      // --core), a stand-in elsewhere
      '@app/core-web': fileURLToPath(
        new URL(
          mode === 'web' ? 'src/generated/core/app_core_web.js' : 'src/lib/backend/noCore.ts',
          import.meta.url,
        ),
      ),
    },
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
    outDir: mode === 'web' ? 'dist-web' : 'dist',
    rollupOptions: {
      // the admin page ships with the server, which serves it at /admin
      // (mode admin: nothing else, the server's web version is the web
      // build); the apps leave it out
      input: {
        ...(mode === 'admin'
          ? {}
          : { main: fileURLToPath(new URL('index.html', import.meta.url)) }),
        ...(platform || mode === 'web'
          ? {}
          : { admin: fileURLToPath(new URL('admin.html', import.meta.url)) }),
      },
      // modules run in the order they are imported, wherever a chunk puts
      // them: the web version's storage is scoped (main.ts's first import)
      // before anything reads it, never another path's
      output: mode === 'web' ? { strictExecutionOrder: true } : {},
    },
  },
}))
