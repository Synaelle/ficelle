// SPDX-License-Identifier: AGPL-3.0-or-later
// ProseMirror pastes plain text as plain paragraphs, so Markdown copied from
// another app stayed as typed (`# Title`, `**bold**`). Text written in
// Markdown is inserted as Markdown instead.

// a heading, a list item, a quote, a fence, a table row or a rule, at a line start
const BLOCK =
  /^ {0,3}(#{1,6}[ \t]|[-*+][ \t]|\d{1,9}[.)][ \t]|>|```|~~~|\|.*\|[ \t]*$|([-*_][ \t]*){3,}$)/m
// bold, strike, code, a link, an image or a link between notes, on one line
const INLINE =
  /\*\*[^*\n]+\*\*|__[^_\n]+__|~~[^~\n]+~~|`[^`\n]+`|!?\[[^\]\n]*\]\([^)\s]+\)|\[\[[^\]\n]+\]\]/

/** Whether pasted plain text is written in Markdown. */
export function looksLikeMarkdown(text: string): boolean {
  return BLOCK.test(text) || INLINE.test(text)
}
