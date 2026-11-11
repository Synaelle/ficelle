// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it } from 'vitest'

import { contactOf, spamScore, validate } from '../src/report'
import { base64, pngOf } from './helpers'

const good = {
  install: '0123456789abcdef0123456789abcdef',
  about: '  Sync stops after sleep  ',
  message: ' It stays on Syncing… until I restart. ',
  version: '0.6.0-beta.1',
  platform: 'windows',
}

describe('validate', () => {
  it('takes a report and trims it', () => {
    const incoming = validate({ ...good, contact: 'Someone@Example.org' })
    expect(incoming).toMatchObject({
      about: 'Sync stops after sleep',
      message: 'It stays on Syncing… until I restart.',
      contact: 'someone@example.org',
      category: '',
      server: null,
      receipt: null,
      images: [],
    })
  })

  it('takes an older app’s category and email', () => {
    const incoming = validate({
      category: 'bug',
      email: 'k@x.org',
      message: 'It crashed twice',
      version: '0.1.9',
      platform: 'android',
    })
    expect(incoming).toMatchObject({ category: 'bug', contact: 'k@x.org', about: null })
  })

  it('says what is wrong', () => {
    expect(validate({ ...good, about: '', message: ' ' })).toBe('the message is empty')
    expect(validate({ ...good, about: 'x'.repeat(121) })).toBe(
      'the about line is over 120 characters',
    )
    expect(validate({ ...good, about: 'a\u0007b' })).toBe('the about line holds control characters')
    expect(validate({ ...good, message: 'a\u0000b' })).toBe('the message holds control characters')
    expect(validate({ ...good, message: 'line\nline\ttab' })).not.toBeTypeOf('string')
    expect(validate({ ...good, version: '1.0<script>' })).toBe('the version is not a version')
    expect(validate({ ...good, platform: 'Windows' })).toBe('the platform is not a platform')
    expect(validate({ ...good, install: 'XYZ' })).toBe('the install id is not 32 lowercase hex')
    expect(validate({ ...good, receipt: 'ab' })).toBe('the receipt is not 64 lowercase hex')
    expect(validate({ ...good, server: 'evil host/' })).toBe('the server is not a host name')
    expect(validate({ ...good, logs: 'a\0b' })).toBe('the logs hold a NUL')
    expect(validate({ ...good, logs: 'x'.repeat(64 * 1024 + 1) })).toBe('the logs are over 64 KB')
    expect(validate({ ...good, contact: 'not an address' })).toBe(
      'the contact is not an email address or a Discord name',
    )
    expect(validate({ ...good, category: 'praise' })).toBeTypeOf('string')
  })

  it('checks images', () => {
    const png = base64(pngOf(320, 200))
    const incoming = validate({ ...good, images: [png, png] })
    expect(typeof incoming !== 'string' && incoming.images.map((i) => i.info.width)).toEqual([
      320, 320,
    ])
    expect(validate({ ...good, images: [png, png, png, png] })).toBe('more than 3 images')
    expect(validate({ ...good, images: ['R0lGODlh'] })).toBe(
      'image 1: not a PNG, JPEG or WebP image',
    )
    expect(validate({ ...good, images: ['%%%'] })).toBe('image 1 is not base64')
    expect(validate({ ...good, images: ['A'.repeat(2_796_204)] })).toBe('image 1 is over 2 MB')
  })
})

describe('contacts', () => {
  it('takes an email address or a Discord name', () => {
    expect(contactOf('k@x.org')).toBe('k@x.org')
    expect(contactOf('@some_name')).toBe('some_name')
    expect(contactOf('Some.Name')).toBe('some.name')
    expect(contactOf('Old Name#1234')).toBeUndefined()
    expect(contactOf('oldname#1234')).toBe('oldname#1234')
    expect(contactOf('a')).toBeUndefined()
    expect(contactOf('@k@x.org')).toBeUndefined()
    expect(contactOf('k@localhost')).toBeUndefined()
  })
})

describe('spam score', () => {
  it('scores links, repeats, symbols and short words', () => {
    expect(spamScore('The editor loses the cursor after an undo.', false)).toEqual({
      score: 0,
      flags: [],
    })
    expect(spamScore('See https://a.example for the details please', false).flags).toEqual(['link'])
    expect(
      spamScore('http://a http://b www.c buy things here now', false).score,
    ).toBeGreaterThanOrEqual(3)
    expect(spamScore('The same words as yesterday, again', true).flags).toEqual(['repeated'])
    expect(spamScore('$$$ ### !!! %%% @@@', false).flags).toContain('symbols')
    expect(spamScore('hi', false).flags).toContain('short')
  })
})
