// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it } from 'vitest'

import { checkImage } from '../src/images'
import { pngOf } from './helpers'

function jpegOf(width: number, height: number): Uint8Array {
  // an APP0 segment first, then padding, then the frame
  return new Uint8Array([
    0xff,
    0xd8,
    0xff,
    0xe0,
    0,
    4,
    0,
    0,
    0xff,
    0xff,
    0xc2,
    0,
    11,
    8,
    height >> 8,
    height & 0xff,
    width >> 8,
    width & 0xff,
    1,
    1,
    0x11,
    0,
  ])
}

function webpOf(chunk: string, body: number[]): Uint8Array {
  const head = new TextEncoder().encode(`RIFF\0\0\0\0WEBP${chunk}`)
  return new Uint8Array([...head, 0, 0, 0, 0, ...body])
}

describe('images', () => {
  it('reads the size of each kind', () => {
    expect(checkImage(pngOf(640, 480))).toEqual({ kind: 'png', width: 640, height: 480 })
    expect(checkImage(jpegOf(800, 600))).toEqual({ kind: 'jpeg', width: 800, height: 600 })
    const lossy = webpOf('VP8 ', [0, 0, 0, 0x9d, 0x01, 0x2a, 300 & 0xff, 300 >> 8, 200, 0])
    expect(checkImage(lossy)).toEqual({ kind: 'webp', width: 300, height: 200 })
    const bits = (100 - 1) | ((50 - 1) << 14)
    const lossless = webpOf('VP8L', [
      0x2f,
      bits & 0xff,
      (bits >> 8) & 0xff,
      (bits >> 16) & 0xff,
      bits >>> 24,
    ])
    expect(checkImage(lossless)).toEqual({ kind: 'webp', width: 100, height: 50 })
    const extended = webpOf('VP8X', [0, 0, 0, 0, 0xff, 0x0f, 0, 0x7f, 0x02, 0])
    expect(checkImage(extended)).toEqual({ kind: 'webp', width: 4096, height: 640 })
  })

  it('refuses other files and wrong sizes', () => {
    const text = (s: string) => new TextEncoder().encode(s)
    expect(checkImage(text('GIF89a....'))).toBeTypeOf('string')
    expect(checkImage(text('<svg xmlns=...>'))).toBeTypeOf('string')
    expect(checkImage(new Uint8Array())).toBeTypeOf('string')
    expect(checkImage(pngOf(5000, 10))).toBe('an image over 4096 pixels a side')
    expect(checkImage(pngOf(10, 0))).toBe('an image without pixels')
    expect(checkImage(pngOf(10, 10).slice(0, 20))).toBeTypeOf('string')
    expect(checkImage(new Uint8Array([0xff, 0xd8, 0xff, 0xda, 0, 2]))).toBeTypeOf('string')
    expect(checkImage(new Uint8Array([0xff, 0xd8, 0xff, 0xe0, 0, 0]))).toBeTypeOf('string')
    // every prefix of a good file is refused or read, never a throw
    const whole = jpegOf(10, 10)
    for (let end = 0; end < whole.length; end++) checkImage(whole.slice(0, end))
  })
})
