// SPDX-License-Identifier: AGPL-3.0-or-later
// Color math for contrast checks, following WCAG 2.x relative luminance.

const HEX = /^#[0-9a-f]{6}$/i

export function isHexColor(value: unknown): value is string {
  return typeof value === 'string' && HEX.test(value)
}

function channel(hex: string, offset: number): number {
  const value = parseInt(hex.slice(offset, offset + 2), 16) / 255
  return value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4
}

export function luminance(hex: string): number {
  return 0.2126 * channel(hex, 1) + 0.7152 * channel(hex, 3) + 0.0722 * channel(hex, 5)
}

/** Contrast ratio between two colors, from 1 to 21. */
export function contrastRatio(a: string, b: string): number {
  const [light, dark] = [luminance(a), luminance(b)].sort((x, y) => y - x) as [number, number]
  return (light + 0.05) / (dark + 0.05)
}

/** Picks whichever of the two candidates reads better on the background. */
export function pickReadable(background: string, candidates: [string, string]): string {
  const [first, second] = candidates
  return contrastRatio(background, first) >= contrastRatio(background, second) ? first : second
}

const hex2 = (value: number) =>
  Math.round(Math.min(255, Math.max(0, value)))
    .toString(16)
    .padStart(2, '0')

/** Blends `from` toward `to`, amount 0 keeps `from` and 1 gives `to`. */
export function mix(from: string, to: string, amount: number): string {
  let out = '#'
  for (const offset of [1, 3, 5]) {
    const a = parseInt(from.slice(offset, offset + 2), 16)
    const b = parseInt(to.slice(offset, offset + 2), 16)
    out += hex2(a + (b - a) * amount)
  }
  return out
}

/**
 * The color moved toward `toward` just enough to reach the contrast on the
 * background, so a hue stays recognisable instead of being swapped out.
 */
export function readableOn(color: string, background: string, toward: string, min: number): string {
  for (let step = 0; step <= 20; step++) {
    const candidate = mix(color, toward, step / 20)
    if (contrastRatio(candidate, background) >= min) return candidate
  }
  return toward
}
