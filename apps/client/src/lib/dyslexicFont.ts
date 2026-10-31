// SPDX-License-Identifier: AGPL-3.0-or-later
// The OpenDyslexic setting: one font for the whole page and the notes,
// whatever the theme says. A theme's css comes after the app's own styles and
// may use !important, so the font is set as !important inside a cascade
// layer declared before anything else on the page: important declarations of
// the earliest layer beat those of later layers and of unlayered css, at any
// specificity. The layer and the family get a random name for each page, so
// a theme can neither add rules to that layer nor replace the font with an
// @font-face of the same name. Code gets OpenDyslexic Mono, so columns
// still line up.
//
// OpenDyslexic itself is downloaded the first time the setting is on, like
// a listed font (lib/fonts.ts), and kept on the device: the app does not
// carry it, to stay small. OpenDyslexic Mono, built for the app and small,
// is bundled.

import mono from '../styles/opendyslexic/opendyslexic-mono-400-normal.woff2?url'
import { dyslexicStatus } from './dyslexicStatus.svelte'
import { type FontPack, loadFontPack } from './fonts'

/** OpenDyslexic from Fontsource, regular, bold and their italics. */
export const OPENDYSLEXIC: FontPack = {
  id: 'opendyslexic',
  family: 'OpenDyslexic',
  files: ['latin-400-normal', 'latin-700-normal', 'latin-400-italic', 'latin-700-italic'],
}

const STYLE_ID = 'dyslexic-font'

const PSEUDOS = [
  '',
  '::before',
  '::after',
  '::first-line',
  '::first-letter',
  '::marker',
  '::placeholder',
  '::file-selector-button',
]

/** What is monospace: code in notes and in the app, and math source. */
const CODE = 'code, pre, kbd, samp, .prose .math'

/** Letters and digits only, safe in a layer name and inside quotes. */
export function randomSuffix(): string {
  const bytes = crypto.getRandomValues(new Uint8Array(8))
  return Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('')
}

const suffix = randomSuffix()

/** The family name OpenDyslexic gets on a page with that suffix. */
export const dyslexicFamily = (id: string) => `OpenDyslexic ${id}`

/** The css that forces the font, with the given random suffix. The mono
 * file comes from `monoUrl`, OpenDyslexic from `loadFontPack`. */
export function dyslexicCss(monoUrl: string, id: string): string {
  const family = dyslexicFamily(id)
  const monoFamily = `OpenDyslexic Mono ${id}`
  const face = (name: string, url: string, weight: number, style: string) =>
    `@font-face { font-family: '${name}'; src: url('${url}') format('woff2'); ` +
    `font-weight: ${weight}; font-style: ${style}; font-display: block; }`
  const stack = `'${family}', system-ui, sans-serif`
  const monoStack = `'${monoFamily}', ui-monospace, monospace`
  // pseudo-elements do not match *, and ::first-line can set its own font.
  // One rule each: a selector a webview does not know drops its whole rule.
  const everything = PSEUDOS.map((p) => `  *${p} { font-family: ${stack} !important; }`)
  return [
    // one weight only, bold code is drawn bolder by the browser
    face(monoFamily, monoUrl, 400, 'normal'),
    `@layer a11y-${id} {`,
    // the variables too, for the theme previews and anything else reading them
    `  :root { --font-ui: ${stack} !important; --font-editor: ${stack} !important;`,
    `    --font-mono: ${monoStack} !important; }`,
    ...everything,
    `  :where(${CODE}), :where(${CODE}) * { font-family: ${monoStack} !important; }`,
    `}`,
  ].join('\n')
}

/**
 * The style element, made first in the head so its layer is declared before
 * any theme's. It stays when the setting is off, empty, so turning it on
 * again keeps that place.
 */
function fontStyle(): HTMLStyleElement {
  let style = document.getElementById(STYLE_ID) as HTMLStyleElement | null
  if (!style) {
    style = document.createElement('style')
    style.id = STYLE_ID
    document.head.prepend(style)
  }
  return style
}

export function applyDyslexicFont(on: boolean) {
  fontStyle().textContent = on ? dyslexicCss(mono, suffix) : ''
  document.documentElement.toggleAttribute('data-dyslexic-font', on)
  dyslexicStatus.failed = false
  if (!on) return void (dyslexicStatus.loading = false)
  // the text shows in the fallback fonts until the files are here
  dyslexicStatus.loading = true
  loadFontPack(OPENDYSLEXIC, true, dyslexicFamily(suffix))
    .then(
      (ok) => (dyslexicStatus.failed = !ok),
      () => (dyslexicStatus.failed = true),
    )
    .finally(() => (dyslexicStatus.loading = false))
}
