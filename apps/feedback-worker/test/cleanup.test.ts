// SPDX-License-Identifier: AGPL-3.0-or-later

import { env } from 'cloudflare:test'
import { describe, expect, it } from 'vitest'

import { DAY } from '../src/limits'
import { cleanup, KEEP_DAYS } from '../src/store'
import { base64, pngOf, sendReport } from './helpers'

describe('the daily cleanup', () => {
  it('removes reports past their time with their images, and old sends', async () => {
    await sendReport({ about: 'Old report with an image', images: [base64(pngOf(4, 4))] })
    await sendReport({ about: 'Recent report kept' })
    const now = Date.now()
    await env.DB.prepare(
      "UPDATE reports SET created_at = ?1 WHERE about = 'Old report with an image'",
    )
      .bind(now - (KEEP_DAYS + 1) * DAY)
      .run()
    await env.DB.prepare('UPDATE sends SET at = ?1')
      .bind(now - 2 * DAY)
      .run()
    await cleanup(env.DB, env.IMAGES, now)
    const left = await env.DB.prepare('SELECT about FROM reports').all<{ about: string }>()
    expect(left.results.map((r) => r.about)).toEqual(['Recent report kept'])
    expect(await env.DB.prepare('SELECT count(*) AS n FROM images').first('n')).toBe(0)
    expect((await env.IMAGES.list()).objects).toEqual([])
    expect(await env.DB.prepare('SELECT count(*) AS n FROM sends').first('n')).toBe(0)
    // challenges used until a minute ago are expired, asks too
    expect(
      await env.DB.prepare('SELECT count(*) AS n FROM used_challenges WHERE expires_at > ?1')
        .bind(now)
        .first('n'),
    ).toBe(2)
  })
})
