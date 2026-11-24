// SPDX-License-Identifier: AGPL-3.0-or-later
// Dates and sizes in the page's language, for the app and the admin page.

import { hour12 } from './clock.svelte'
import { currentLocale, t } from './i18n.svelte'

export function dateTime(ms: number | null | undefined): string {
  if (!ms) return '–'
  return new Intl.DateTimeFormat(currentLocale(), {
    dateStyle: 'medium',
    timeStyle: 'short',
    hour12: hour12(),
  }).format(ms)
}

export function date(ms: number | null | undefined): string {
  if (!ms) return '–'
  return new Intl.DateTimeFormat(currentLocale(), { dateStyle: 'medium' }).format(ms)
}

/** "5 minutes ago", "in 2 days". */
export function relative(ms: number | null | undefined, now = Date.now()): string {
  if (!ms) return t('common.never')
  const rtf = new Intl.RelativeTimeFormat(currentLocale(), { numeric: 'auto' })
  const seconds = Math.round((ms - now) / 1000)
  const steps: [Intl.RelativeTimeFormatUnit, number][] = [
    ['second', 60],
    ['minute', 60],
    ['hour', 24],
    ['day', 30],
    ['month', 12],
    ['year', Infinity],
  ]
  let value = seconds
  for (const [unit, size] of steps) {
    if (Math.abs(value) < size) return rtf.format(value, unit)
    value = Math.round(value / size)
  }
  return rtf.format(value, 'year')
}

const UNITS = ['byte', 'kilobyte', 'megabyte', 'gigabyte', 'terabyte'] as const

/** The unit `n` bytes read best in, at least kilobytes, and its size. */
function unitOf(n: number): [(typeof UNITS)[number], number] {
  let unit = 1
  while (n >= 1024 ** (unit + 1) && unit < UNITS.length - 1) unit += 1
  return [UNITS[unit] ?? 'kilobyte', 1024 ** unit]
}

/** "33 bytes", "12 MB", "1.5 GB", in the page's language. */
export function bytes(n: number): string {
  // below a kilobyte, as many bytes: a fraction of one rounded to "0 kB",
  // and the short unit reads "33 byte"
  if (n < 1024)
    return new Intl.NumberFormat(currentLocale(), {
      style: 'unit',
      unit: 'byte',
      unitDisplay: 'long',
    }).format(n)
  const [unit, size] = unitOf(n)
  const value = n / size
  return new Intl.NumberFormat(currentLocale(), {
    style: 'unit',
    unit,
    unitDisplay: 'short',
    maximumFractionDigits: value < 10 ? 1 : 0,
  }).format(value)
}

/** "12 of 340 MB": how much of a file moved, both in the unit of the whole. */
export function bytesOf(done: number, total: number): string {
  if (total < 1024) return t('files.progress', { done, total: bytes(total) })
  const [, size] = unitOf(total)
  const digits = total / size < 10 ? 1 : 0
  const part = new Intl.NumberFormat(currentLocale(), {
    maximumFractionDigits: digits,
  }).format(done / size)
  return t('files.progress', { done: part, total: bytes(total) })
}

/** "15 minutes", "3 hours", "31 days", rounded, in the page's language. */
export function duration(ms: number): string {
  const minutes = Math.round(Math.abs(ms) / 60_000)
  const [value, unit] =
    minutes < 120
      ? [minutes, 'minute']
      : minutes < 48 * 60
        ? [Math.round(minutes / 60), 'hour']
        : [Math.round(minutes / (24 * 60)), 'day']
  return new Intl.NumberFormat(currentLocale(), {
    style: 'unit',
    unit,
    unitDisplay: 'long',
  }).format(value)
}
