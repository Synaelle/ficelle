// SPDX-License-Identifier: AGPL-3.0-or-later
// The report form's limits are the server's (crates/protocol/src/feedback.rs):
// a form that let more through would have its reports refused.

import { describe, expect, it } from 'vitest'
import rust from '../../../../crates/protocol/src/feedback.rs?raw'
import * as feedback from './feedback'

/** A `pub const NAME: type = value;` of the Rust file, as a number. */
function rustConst(name: string): number {
  const found = new RegExp(`pub const ${name}: \\w+ = ([^;]+);`).exec(rust)
  if (!found?.[1]) throw new Error(`${name} is not in feedback.rs`)
  return found[1]
    .replaceAll('_', '')
    .split('*')
    .reduce((product, factor) => product * Number(factor.trim()), 1)
}

describe('feedback limits', () => {
  it('are the server’s', () => {
    for (const name of [
      'MAX_ABOUT',
      'MAX_MESSAGE',
      'MAX_CONTACT',
      'MAX_LOGS',
      'MAX_IMAGES',
      'MAX_IMAGE_BYTES',
      'MAX_IMAGE_SIDE',
    ] as const) {
      expect(feedback[name], name).toBe(rustConst(name))
    }
    expect(feedback.MAX_SENT).toBeLessThanOrEqual(rustConst('MAX_ASKED'))
  })
})
