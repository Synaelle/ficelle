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
  if (!ms) return t('admin.never')
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
  const units = ['byte', 'kilobyte', 'megabyte', 'gigabyte', 'terabyte'] as const
  let value = n / 1024
  let unit = 1
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024
    unit += 1
  }
  return new Intl.NumberFormat(currentLocale(), {
    style: 'unit',
    unit: units[unit],
    unitDisplay: 'short',
    maximumFractionDigits: value < 10 ? 1 : 0,
  }).format(value)
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
