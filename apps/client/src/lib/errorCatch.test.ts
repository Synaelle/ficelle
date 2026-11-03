// SPDX-License-Identifier: AGPL-3.0-or-later
// Which errors the notice shows: real ones, once each, one at a time.

import { describe, expect, it } from 'vitest'
import { errorCatch } from './errorCatch.svelte'

describe('errorCatch', () => {
  it('shows an error once, one at a time, and leaves browser noise out', () => {
    errorCatch.catch(new Error('ResizeObserver loop completed with undelivered notifications.'))
    expect(errorCatch.caught).toBeNull()

    errorCatch.catch(new TypeError('x is undefined'))
    expect(errorCatch.caught).toBe('TypeError: x is undefined')

    // a second one waits behind the first, and is not shown later either
    errorCatch.catch(new Error('another'))
    expect(errorCatch.caught).toBe('TypeError: x is undefined')
    errorCatch.dismiss()
    errorCatch.catch(new Error('another'))
    errorCatch.catch(new TypeError('x is undefined'))
    expect(errorCatch.caught).toBeNull()

    errorCatch.catch({ kind: 'database', message: 'refused' })
    expect(errorCatch.caught).toBe('database: refused')
  })
})
