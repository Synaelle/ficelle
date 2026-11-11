// SPDX-License-Identifier: AGPL-3.0-or-later

import { env } from 'cloudflare:test'
import { describe, expect, it } from 'vitest'

import { CHALLENGE_LIFE, makeChallenge, takeChallenge } from '../src/challenge'
import { ASKS_PER_MINUTE, INSTANCE_PER_DAY, PER_HOUR } from '../src/limits'
import { base64, call, freshIp, pngOf, post, receiptOf, sendReport, solve, solved } from './helpers'

describe('the protocol', () => {
  it('needs the protocol header', async () => {
    const response = await call('/api/v1/feedback/challenge', { method: 'POST', protocol: null })
    expect(response.status).toBe(400)
    expect(await response.json()).toMatchObject({ error: 'protocolMissing' })
    const newer = await call('/api/v1/feedback/challenge', { method: 'POST', protocol: '2' })
    expect(newer.status).toBe(426)
    expect(await newer.json()).toMatchObject({ error: 'updateServer' })
  })

  it('answers any origin, and its preflight without the header', async () => {
    const preflight = await call('/api/v1/feedback', { method: 'OPTIONS', protocol: null })
    expect(preflight.status).toBe(204)
    expect(preflight.headers.get('access-control-allow-origin')).toBe('*')
    expect(preflight.headers.get('access-control-allow-headers')).toContain('x-protocol-version')
    const response = await post('/api/v1/feedback/challenge', {}, freshIp())
    expect(response.headers.get('access-control-allow-origin')).toBe('*')
    expect(response.headers.get('access-control-expose-headers')).toBe('retry-after')
  })

  it('knows no other path', async () => {
    expect((await call('/api/v1/feedback/info')).status).toBe(404)
    expect((await call('/')).status).toBe(404)
  })
})

describe('challenges', () => {
  it('are sealed, expire and are taken once', async () => {
    const now = Date.now()
    const made = await makeChallenge(env.DB, now, 8)
    expect(made).toMatchObject({ bits: 8, expiresAt: now + CHALLENGE_LIFE })
    const nonce = await solve(made.challenge, 8)
    expect(await takeChallenge(env.DB, now + CHALLENGE_LIFE, made.challenge, nonce)).toBe('expired')
    // a nonce that does not answer: the first one before the solution
    let wrong = 0
    while (wrong === nonce) wrong++
    if (wrong < nonce) {
      expect(await takeChallenge(env.DB, now, made.challenge, wrong)).toBe('notSolved')
    }
    expect(await takeChallenge(env.DB, now, made.challenge, nonce)).toBeUndefined()
    expect(await takeChallenge(env.DB, now, made.challenge, nonce)).toBe('used')
    const tampered = `${made.challenge.slice(0, 30)}${made.challenge[30] === 'A' ? 'B' : 'A'}${made.challenge.slice(31)}`
    expect(await takeChallenge(env.DB, now, tampered, nonce)).toBe('unknown')
    expect(await takeChallenge(env.DB, now, 'nope', 0)).toBe('unknown')
  })

  it('ask more work of a busy address', async () => {
    const ip = freshIp()
    const first = await (await post('/api/v1/feedback/challenge', {}, ip)).json<{ bits: number }>()
    expect(first.bits).toBe(8)
    expect((await sendReport({}, ip)).status).toBe(204)
    const second = await (await post('/api/v1/feedback/challenge', {}, ip)).json<{ bits: number }>()
    expect(second.bits).toBe(10)
  })

  it('are limited a minute per address', async () => {
    const ip = freshIp()
    for (let i = 0; i < ASKS_PER_MINUTE; i++) {
      expect((await post('/api/v1/feedback/challenge', {}, ip)).status).toBe(200)
    }
    const refused = await post('/api/v1/feedback/challenge', {}, ip)
    expect(refused.status).toBe(429)
    expect(Number(refused.headers.get('retry-after'))).toBeGreaterThan(0)
    expect(await refused.json()).toMatchObject({ error: 'rateLimited' })
    // an IPv6 address counts by its /64
    const v6 = '2001:db8:aa:bb::1'
    for (let i = 0; i < ASKS_PER_MINUTE; i++) {
      await post('/api/v1/feedback/challenge', {}, i % 2 ? v6 : '2001:db8:aa:bb:ffff::9')
    }
    expect((await post('/api/v1/feedback/challenge', {}, '2001:db8:aa:bb:1:2:3:4')).status).toBe(
      429,
    )
  })
})

describe('reports', () => {
  it('are kept with their images', async () => {
    const png = pngOf(1280, 720)
    const response = await sendReport({
      contact: 'tester@example.org',
      logs: 'WARN sync: retry in 5 s',
      images: [base64(png), base64(pngOf(64, 64)), base64(pngOf(10, 20))],
      receipt: await receiptOf('secret-one'),
    })
    expect(response.status).toBe(204)
    // an empty answer the edge must not gzip, see noContent
    expect(response.headers.get('content-encoding')).toBe('identity')
    const row = await env.DB.prepare(
      'SELECT * FROM reports ORDER BY created_at DESC LIMIT 1',
    ).first<Record<string, unknown>>()
    const id = String(row?.id)
    expect(row).toMatchObject({
      status: 'new',
      about: 'The table bar moves after a resize',
      contact: 'tester@example.org',
      platform: 'linux',
      logs: 'WARN sync: retry in 5 s',
      score: 0,
    })
    const images = await env.DB.prepare('SELECT * FROM images WHERE report = ?1 ORDER BY position')
      .bind(id)
      .all()
    expect(images.results.map((i) => [i.kind, i.width, i.height])).toEqual([
      ['png', 1280, 720],
      ['png', 64, 64],
      ['png', 10, 20],
    ])
    const stored = await env.IMAGES.get(`${id}/0`)
    expect(stored?.httpMetadata?.contentType).toBe('image/png')
    expect(new Uint8Array((await stored?.arrayBuffer()) ?? new ArrayBuffer(0))).toEqual(png)
    // no address in the report, only the keyed hash in the day's sends
    const send = await env.DB.prepare('SELECT address FROM sends ORDER BY at DESC LIMIT 1').first<{
      address: string
    }>()
    expect(send?.address).toMatch(/^[0-9a-f]{32}$/)
  })

  it('go to spam without the sender knowing', async () => {
    const response = await sendReport({
      about: 'cheap',
      message: 'http://a.example http://b.example www.c.example',
    })
    expect(response.status).toBe(204)
    const row = await env.DB.prepare(
      "SELECT status, flags FROM reports WHERE about = 'cheap'",
    ).first()
    expect(row).toMatchObject({ status: 'spam' })
  })

  it('are refused without a good challenge', async () => {
    const ip = freshIp()
    const proof = await solved(ip)
    const body = {
      ...proof,
      about: 'Fine',
      message: 'A fine report about the editor',
      version: '1',
      platform: 'linux',
    }
    expect((await post('/api/v1/feedback', body, ip)).status).toBe(204)
    const replay = await post('/api/v1/feedback', body, ip)
    expect(replay.status).toBe(400)
    expect(await replay.json()).toMatchObject({ error: 'challengeInvalid' })
    const unsolved = await post('/api/v1/feedback', { ...body, challenge: 'made-up' }, ip)
    expect(await unsolved.json()).toMatchObject({ error: 'challengeInvalid' })
  })

  it('are refused when malformed or too large', async () => {
    const ip = freshIp()
    expect(await (await post('/api/v1/feedback', 'text', ip)).json()).toMatchObject({
      error: 'invalid',
    })
    const broken = await call('/api/v1/feedback', { method: 'POST', body: '{', ip })
    expect(broken.status).toBe(400)
    const empty = await sendReport({ about: '', message: '' })
    expect(await empty.json()).toMatchObject({ error: 'invalid', message: 'the message is empty' })
    const huge = await call('/api/v1/feedback', {
      method: 'POST',
      body: new Uint8Array(9 * 1024 * 1024 + 1),
      ip,
    })
    expect(huge.status).toBe(413)
  })

  it('are limited per install, per server and in all', async () => {
    const install = 'ffffffffffffffffffffffffffffffff'
    const ip = freshIp()
    await env.DB.batch(
      Array.from({ length: PER_HOUR }, () =>
        env.DB.prepare(
          "INSERT INTO sends (at, address, install, message_hash) VALUES (?1, 'other', ?2, 'h')",
        ).bind(Date.now() - 1000, install),
      ),
    )
    const capped = await sendReport({ install }, ip)
    expect(capped.status).toBe(429)
    expect(Number(capped.headers.get('retry-after'))).toBeGreaterThan(3000)
    await env.DB.batch(
      Array.from({ length: PER_HOUR }, () =>
        env.DB.prepare(
          "INSERT INTO sends (at, address, server, message_hash) VALUES (?1, 'other', 'busy.example', 'h')",
        ).bind(Date.now() - 1000),
      ),
    )
    expect((await sendReport({ server: 'Busy.Example' })).status).toBe(429)
    expect((await sendReport({ server: 'quiet.example' })).status).toBe(204)
    await env.DB.prepare(
      `WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < ?1)
       INSERT INTO sends (at, address, message_hash) SELECT ?2, 'many', 'h' FROM n`,
    )
      .bind(INSTANCE_PER_DAY, Date.now())
      .run()
    expect((await sendReport({})).status).toBe(429)
  })
})

describe('answers', () => {
  it('tell a sender how their reports stand', async () => {
    await sendReport({ about: 'First one here', receipt: await receiptOf('s1') })
    await sendReport({ about: 'Second one here', receipt: await receiptOf('s2') })
    await sendReport({
      about: 'spam',
      message: 'http://x http://y http://z',
      receipt: await receiptOf('s3'),
    })
    await env.DB.prepare(
      "UPDATE reports SET status = 'done', answer = 'Fixed in 0.6.1' WHERE about = 'First one here'",
    ).run()
    const response = await post(
      '/api/v1/feedback/answers',
      { secrets: ['s1', 's2', 's3', 'unknown'] },
      freshIp(),
    )
    expect(response.status).toBe(200)
    expect(await response.json()).toEqual({
      reports: [
        { secret: 's1', state: 'done', answer: 'Fixed in 0.6.1' },
        { secret: 's2', state: 'received', answer: '' },
        { secret: 's3', state: 'received', answer: '' },
      ],
    })
  })

  it('refuse too many secrets', async () => {
    const response = await post(
      '/api/v1/feedback/answers',
      { secrets: Array.from({ length: 51 }, (_, i) => `s${i}`) },
      freshIp(),
    )
    expect(response.status).toBe(400)
    expect((await post('/api/v1/feedback/answers', { secrets: [''] }, freshIp())).status).toBe(400)
    expect(
      await (await post('/api/v1/feedback/answers', { secrets: [] }, freshIp())).json(),
    ).toEqual({ reports: [] })
  })
})
