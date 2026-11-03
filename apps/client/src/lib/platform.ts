// SPDX-License-Identifier: AGPL-3.0-or-later
// The system this build is for, fixed when it is built (vite.config.ts reads
// TAURI_ENV_PLATFORM from the tauri cli): never guessed from the browser, a
// narrow window is not a phone. Code behind a check for another system is
// left out of the build, so the phone app carries no Windows Hello or quick
// note code and the desktop one no phone code. "web" is the browser during
// development and the tests.

export const PLATFORM = __APP_PLATFORM__

export const ANDROID = PLATFORM === 'android'
export const IOS = PLATFORM === 'ios'
/** A phone or a tablet. */
export const MOBILE = ANDROID || IOS
export const WINDOWS = PLATFORM === 'windows'
export const MAC = PLATFORM === 'darwin'
export const LINUX = PLATFORM === 'linux'
/** Windows, macOS or Linux. */
export const DESKTOP = WINDOWS || MAC || LINUX
