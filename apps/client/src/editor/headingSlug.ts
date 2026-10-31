// SPDX-License-Identifier: AGPL-3.0-or-later
// A heading's anchor as GitHub makes it, so `[see](#my-heading)` written for
// GitHub jumps to the same heading here: lower case, punctuation dropped,
// each space a dash. Letters of every language stay.

export function headingSlug(text: string): string {
  return text
    .trim()
    .toLowerCase()
    .replace(/[^\p{L}\p{M}\p{N}\s_-]/gu, '')
    .replace(/\s/g, '-')
}
