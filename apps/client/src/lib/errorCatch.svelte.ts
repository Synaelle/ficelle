// SPDX-License-Identifier: AGPL-3.0-or-later
// Errors nothing handled, the ones that mean a bug: a small notice offers to
// report it (lib/ErrorReport.svelte). Failures the app expects, like being
// offline or a wrong password, are handled where they happen and never get
// here. Each error shows once a run, one notice at a time.

import { screenshot } from './screenshot'

/** Browser noise that is no bug of the app. */
const NOISE = [/ResizeObserver loop/i, /^Script error\.?$/]

class ErrorCatch {
  /** The error the notice is about, in one line. */
  caught: string | null = $state(null)
  /** Where it happened, the first calls, for the report. */
  where = ''
  /** The report form is open. */
  open = $state(false)
  /** The app as it showed when Report it was pressed, sent only if ticked. */
  shot: File | null = $state(null)
  /** The notice hides while the picture is taken. */
  shooting = $state(false)
  private seen = new Set<string>()

  catch(error: unknown) {
    const text = describe(error)
    if (!text || NOISE.some((n) => n.test(text)) || this.seen.has(text)) return
    this.seen.add(text)
    // the one already shown, or being reported, stays
    if (this.caught || this.open) return
    this.caught = text
    this.where = places(error)
  }

  /** Report it: the picture first, without the notice, then the form. Dev
   * and preview builds open their own report instead, so test errors never
   * reach the project's server. */
  async report() {
    if (__APP_DEV_TOOLS__) {
      window.dispatchEvent(new Event('app-dev-report'))
      this.caught = null
      return
    }
    this.shooting = true
    await frame()
    await frame()
    // notes kept out of screenshots stay out of this one too
    this.shot = window.AppSystem?.hidden?.() ? null : await screenshot()
    this.shooting = false
    this.open = true
  }

  dismiss() {
    this.caught = null
    this.open = false
    this.shot = null
  }
}

const frame = () => new Promise((done) => requestAnimationFrame(() => done(null)))

function describe(error: unknown): string {
  if (error instanceof Error) return `${error.name}: ${error.message}`.trim()
  if (typeof error === 'object' && error !== null && 'kind' in error) {
    const e = error as { kind: unknown; message?: unknown }
    return `${String(e.kind)}: ${String(e.message ?? '')}`.trim()
  }
  return String(error ?? '').trim()
}

/** The first places of an error's stack. */
function places(error: unknown): string {
  if (!(error instanceof Error) || !error.stack) return ''
  return error.stack
    .split('\n')
    .slice(1, 6)
    .map((line) => line.trim())
    .join('\n')
}

export const errorCatch = new ErrorCatch()

let listening = false

/** Starts listening, once. */
export function catchErrors() {
  if (listening) return
  listening = true
  window.addEventListener('error', (e) => errorCatch.catch(e.error ?? e.message))
  window.addEventListener('unhandledrejection', (e) => errorCatch.catch(e.reason))
}
