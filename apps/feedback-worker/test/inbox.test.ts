// SPDX-License-Identifier: AGPL-3.0-or-later

import { env } from 'cloudflare:test'
import { describe, expect, it } from 'vitest'

import { verifyAccessToken } from '../src/access'
import worker from '../src/index'
import { BASE, base64, call, freshIp, pngOf, post, receiptOf, sendReport } from './helpers'

async function latestId(): Promise<string> {
  const id = await env.DB.prepare(
    'SELECT id FROM reports ORDER BY created_at DESC LIMIT 1',
  ).first<string>('id')
  if (!id) throw new Error('no report')
  return id
}

function form(path: string, fields: Record<string, string>, origin = BASE): Promise<Response> {
  return call(path, {
    method: 'POST',
    body: new URLSearchParams(fields).toString(),
    headers: { 'content-type': 'application/x-www-form-urlencoded', origin },
    protocol: null,
  })
}

async function page(path: string): Promise<string> {
  const response = await call(path, { protocol: null })
  expect(response.status).toBe(200)
  expect(response.headers.get('content-security-policy')).toContain("default-src 'none'")
  return response.text()
}

describe('the inbox', () => {
  it('lists reports, spam only when asked for', async () => {
    await sendReport({
      about: 'Search misses accents',
      message: 'Searching for cafe does not find café.',
    })
    await sendReport({
      about: 'buy',
      message: 'http://a.example http://b.example http://c.example',
    })
    const inbox = await page('/inbox')
    expect(inbox).toContain('Search misses accents')
    expect(inbox).not.toContain('http://a.example')
    const spam = await page('/inbox?status=spam')
    expect(spam).toContain('buy')
    expect(spam).not.toContain('Search misses accents')
    expect(await page('/inbox?status=done')).toContain('Nothing done yet.')
  })

  it('shows a report whole, its images and escapes what was sent', async () => {
    const png = pngOf(800, 600)
    await sendReport({
      about: '<b>Bold</b> about',
      message: 'Words <script>alert(1)</script> here',
      logs: 'ERROR one\nWARN two',
      contact: 'someone@example.org',
      images: [base64(png), base64(pngOf(20, 20)), base64(pngOf(30, 30))],
    })
    const id = await latestId()
    const html = await page(`/inbox/${id}`)
    expect(html).toContain('&lt;b&gt;Bold&lt;/b&gt; about')
    expect(html).not.toContain('<script>')
    expect(html).toContain('mailto:someone@example.org')
    expect(html).toContain(`/inbox/${id}/images/2`)
    expect(html).toContain('800×600')
    const image = await call(`/inbox/${id}/images/0`, { protocol: null })
    expect(image.headers.get('content-type')).toBe('image/png')
    expect(image.headers.get('x-content-type-options')).toBe('nosniff')
    expect(new Uint8Array(await image.arrayBuffer())).toEqual(png)
    expect((await call(`/inbox/${id}/images/3`, { protocol: null })).status).toBe(404)
    expect((await call(`/inbox/${'0'.repeat(32)}`, { protocol: null })).status).toBe(404)
  })

  it('answers, and the answer reaches the sender’s app', async () => {
    await sendReport({ about: 'Export to PDF is blank', receipt: await receiptOf('kept-secret') })
    const id = await latestId()
    const saved = await form(`/inbox/${id}`, {
      action: 'save',
      status: 'done',
      answer: ' Fixed in 0.6.1, thanks. ',
    })
    expect(saved.status).toBe(303)
    expect(saved.headers.get('location')).toBe(`/inbox/${id}?saved=1`)
    expect(await page(`/inbox/${id}?saved=1`)).toContain('Fixed in 0.6.1, thanks.')
    const answers = await post('/api/v1/feedback/answers', { secrets: ['kept-secret'] }, freshIp())
    expect(await answers.json()).toEqual({
      reports: [{ secret: 'kept-secret', state: 'done', answer: 'Fixed in 0.6.1, thanks.' }],
    })
    const tooLong = await form(`/inbox/${id}`, {
      action: 'save',
      status: 'open',
      answer: 'x'.repeat(2001),
    })
    expect(tooLong.status).toBe(400)
  })

  it('marks spam, and deletes a report with its images', async () => {
    await sendReport({
      about: 'Two images attached',
      images: [base64(pngOf(5, 5)), base64(pngOf(6, 6))],
    })
    const id = await latestId()
    expect((await form(`/inbox/${id}`, { action: 'spam' })).status).toBe(303)
    expect(await page('/inbox')).not.toContain('Two images attached')
    expect(await page('/inbox?status=spam')).toContain('Two images attached')
    expect((await form(`/inbox/${id}`, { action: 'notSpam' })).status).toBe(303)
    expect(await page('/inbox?status=new')).toContain('Two images attached')
    const deleted = await form(`/inbox/${id}`, { action: 'delete' })
    expect(deleted.headers.get('location')).toBe('/inbox')
    expect(await env.DB.prepare('SELECT count(*) AS n FROM reports').first('n')).toBe(0)
    expect(await env.DB.prepare('SELECT count(*) AS n FROM images').first('n')).toBe(0)
    expect((await env.IMAGES.list()).objects).toEqual([])
  })

  it('takes forms from itself only', async () => {
    await sendReport({ about: 'A report to keep' })
    const id = await latestId()
    expect(
      (await form(`/inbox/${id}`, { action: 'delete' }, 'https://elsewhere.example')).status,
    ).toBe(403)
    expect(await env.DB.prepare('SELECT count(*) AS n FROM reports').first('n')).toBe(1)
  })

  it('stays closed without Access away from a local run', async () => {
    const outside = await worker.fetch(new Request('https://feedback.example/inbox'), env)
    expect(outside.status).toBe(403)
    const withAccess = {
      ...env,
      ACCESS_TEAM_DOMAIN: 'team.cloudflareaccess.com',
      ACCESS_AUD: 'aud',
    }
    const unsigned = await worker.fetch(new Request(`${BASE}/inbox`), withAccess)
    expect(unsigned.status).toBe(403)
  })
})

describe('Access tokens', () => {
  const team = 'team.cloudflareaccess.com'
  const aud = 'a1b2c3'

  async function signer() {
    const pair = (await crypto.subtle.generateKey(
      {
        name: 'RSASSA-PKCS1-v1_5',
        modulusLength: 2048,
        publicExponent: new Uint8Array([1, 0, 1]),
        hash: 'SHA-256',
      },
      true,
      ['sign', 'verify'],
    )) as CryptoKeyPair
    const jwk = {
      ...((await crypto.subtle.exportKey('jwk', pair.publicKey)) as JsonWebKey),
      kid: 'k1',
    }
    const part = (value: unknown) =>
      new TextEncoder()
        .encode(JSON.stringify(value))
        .toBase64({ alphabet: 'base64url', omitPadding: true })
    const sign = async (claims: Record<string, unknown>, kid = 'k1') => {
      const unsigned = `${part({ alg: 'RS256', kid })}.${part(claims)}`
      const signature = await crypto.subtle.sign(
        'RSASSA-PKCS1-v1_5',
        pair.privateKey,
        new TextEncoder().encode(unsigned),
      )
      return `${unsigned}.${new Uint8Array(signature).toBase64({ alphabet: 'base64url', omitPadding: true })}`
    }
    return { jwk, sign }
  }

  it('are taken signed, for this application, in time', async () => {
    const { jwk, sign } = await signer()
    const now = Date.now()
    const claims = {
      aud: [aud],
      iss: `https://${team}`,
      exp: now / 1000 + 600,
      email: 'me@example.org',
    }
    expect(await verifyAccessToken(await sign(claims), [jwk], team, aud, now)).toBe(
      'me@example.org',
    )
    expect(
      await verifyAccessToken(await sign({ ...claims, aud: ['other'] }), [jwk], team, aud, now),
    ).toBeUndefined()
    expect(
      await verifyAccessToken(
        await sign({ ...claims, iss: 'https://evil.example' }),
        [jwk],
        team,
        aud,
        now,
      ),
    ).toBeUndefined()
    expect(
      await verifyAccessToken(
        await sign({ ...claims, exp: now / 1000 - 1 }),
        [jwk],
        team,
        aud,
        now,
      ),
    ).toBeUndefined()
    expect(await verifyAccessToken(await sign(claims, 'k2'), [jwk], team, aud, now)).toBeUndefined()
    const token = await sign(claims)
    const forged = `${token.slice(0, token.lastIndexOf('.'))}.${'A'.repeat(342)}`
    expect(await verifyAccessToken(forged, [jwk], team, aud, now)).toBeUndefined()
    expect(await verifyAccessToken('not.a.token', [jwk], team, aud, now)).toBeUndefined()
  })
})
