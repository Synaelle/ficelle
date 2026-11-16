// SPDX-License-Identifier: AGPL-3.0-or-later
// What a link dialog took as the address. A web address typed without its
// https:// (example.org/hours) would otherwise be saved as a path inside the
// notes, which leads nowhere.

/** `typed`, with https:// in front when it is a bare domain. Anything with a
 * scheme, a path, an anchor or a mail address stays as it is. */
export function linkAddress(typed: string): string {
  const href = typed.trim()
  if (/^[a-z][a-z0-9+.-]*:(?!\d)/i.test(href) || /^[/#.?]/.test(href)) return href
  if (href.includes('@') && !href.includes('/')) return `mailto:${href}`
  return /^[\w-]+(\.[\w-]+)+(:\d+)?([/?#]|$)/.test(href) ? `https://${href}` : href
}
