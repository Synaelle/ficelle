// SPDX-License-Identifier: AGPL-3.0-or-later
// The system this build is for, fixed when it is built (vite.config.ts reads
// TAURI_ENV_PLATFORM from the tauri cli): never guessed from the browser, a
// narrow window is not a phone. Code behind a check for another system is
// left out of the build, so the phone app carries no Windows Hello or quick
// note code and the desktop one no phone code. "web" is the web version, and
// the browser during development and the tests.

export const PLATFORM = __APP_PLATFORM__
/** The web version people use, never the browser preview of development
 * and the tests, which shares PLATFORM "web". */
export const WEB = __APP_WEB__

export const ANDROID = PLATFORM === 'android'
/** A phone or a tablet. */
export const MOBILE = ANDROID
/** A phone or a tablet's layout and editing: the Android app, and the web
 * version on a touch screen, which a narrow mouse window is not. */
export const HANDHELD =
  ANDROID || (PLATFORM === 'web' && window.matchMedia('(pointer: coarse)').matches)
export const WINDOWS = PLATFORM === 'windows'
export const MAC = PLATFORM === 'darwin'
export const LINUX = PLATFORM === 'linux'
/** Windows, macOS or Linux. */
export const DESKTOP = WINDOWS || MAC || LINUX
