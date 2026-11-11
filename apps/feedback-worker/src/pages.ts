// SPDX-License-Identifier: AGPL-3.0-or-later
// The inbox's pages, plain HTML with no script, in the app's own palette
// (the Ficelle theme in packages/ui) and its square corners.

import type { Status } from './store'

export function escape(text: string): string {
  return text.replace(
    /[&<>"']/g,
    (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c] ?? c,
  )
}

const STYLE = `
:root {
  color-scheme: light dark;
  --bg: #F1EFEA; --surface: #FBFAF7; --surface2: #E5E2DB; --border: #CBC6BC;
  --border-strong: #8F887F; --text: #161413; --muted: #5E5853; --accent: #C1271B;
  --accent-text: #C1271B; --accent-on: #FFF6F0; --danger: #A3341F; --success: #3D6B35;
  --warning: #8A5A00; --selection: #F5D5CE;
  --ui: "Hanken Grotesk", system-ui, -apple-system, "Segoe UI", sans-serif;
  --mono: "IBM Plex Mono", ui-monospace, "Cascadia Mono", Menlo, monospace;
}
@media (prefers-color-scheme: dark) {
  :root {
    --bg: #0E0D0D; --surface: #161414; --surface2: #1F1C1B; --border: #2E2A28;
    --border-strong: #6B625C; --text: #E6DFD8; --muted: #9A918A; --accent: #E44C3E;
    --accent-text: #EA5A4C; --accent-on: #140B09; --danger: #FF7A5C; --success: #8DB07A;
    --warning: #D9A441; --selection: #3A1915;
  }
}
* { box-sizing: border-box; }
::selection { background: var(--selection); }
body { margin: 0; background: var(--bg); color: var(--text); font: 15px/1.5 var(--ui); }
a { color: var(--accent-text); }
a:focus-visible, button:focus-visible, select:focus-visible, textarea:focus-visible, summary:focus-visible {
  outline: 2px solid var(--accent); outline-offset: 2px;
}
.wrap { max-width: 960px; margin: 0 auto; padding: 0 16px 48px; }
header { display: flex; justify-content: space-between; align-items: baseline; gap: 12px;
  padding: 16px 0 12px; border-bottom: 1px solid var(--border); }
header .name { font-weight: 700; font-size: 18px; color: var(--text); text-decoration: none; }
header .who { color: var(--muted); font-size: 13px; overflow-wrap: anywhere; }
nav { display: flex; flex-wrap: wrap; gap: 4px 16px; padding: 12px 0; border-bottom: 1px solid var(--border); }
nav a { color: var(--muted); text-decoration: none; padding: 4px 0; border-bottom: 2px solid transparent; }
nav a[aria-current="page"] { color: var(--text); border-bottom-color: var(--accent); font-weight: 600; }
nav .n { font-family: var(--mono); font-size: 13px; margin-left: 4px; }
.list { list-style: none; margin: 0; padding: 0; }
.list li { border-bottom: 1px solid var(--border); }
.list a { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 4px 24px;
  padding: 12px 8px; margin: 0 -8px; color: inherit; text-decoration: none; }
.list a:hover { background: var(--surface); }
.list .about { font-weight: 600; overflow-wrap: anywhere; }
.list .excerpt { grid-column: 1; color: var(--muted); overflow: hidden; text-overflow: ellipsis;
  white-space: nowrap; }
.list .meta { grid-column: 2; grid-row: 1 / span 2; text-align: right; color: var(--muted);
  font-size: 13px; font-family: var(--mono); white-space: nowrap; }
.tags { display: flex; gap: 8px; justify-content: flex-end; flex-wrap: wrap; }
.tag { font-family: var(--mono); font-size: 12px; padding: 0 4px; border: 1px solid var(--border-strong);
  border-radius: 2px; color: var(--muted); }
.tag.new { color: var(--accent-text); border-color: var(--accent-text); }
.tag.done { color: var(--success); border-color: var(--success); }
.tag.spam { color: var(--warning); border-color: var(--warning); }
.empty { padding: 48px 0; color: var(--muted); }
.more { display: inline-block; margin-top: 16px; }
.back { display: inline-block; margin: 16px 0 8px; }
h1 { font-size: 22px; line-height: 1.3; margin: 8px 0 16px; overflow-wrap: anywhere; }
h2 { font-size: 15px; margin: 32px 0 8px; }
dl.facts { display: grid; grid-template-columns: max-content minmax(0, 1fr); gap: 4px 16px; margin: 0;
  font-size: 13px; }
dl.facts dt { color: var(--muted); }
dl.facts dd { margin: 0; font-family: var(--mono); overflow-wrap: anywhere; }
.message { white-space: pre-wrap; overflow-wrap: anywhere; background: var(--surface);
  border: 1px solid var(--border); border-radius: 2px; padding: 12px 16px; margin: 0; font: inherit; }
.message.none { color: var(--muted); }
.images { display: grid; gap: 16px; }
figure { margin: 0; }
figure img { display: block; max-width: 100%; height: auto; border: 1px solid var(--border);
  background: var(--surface2); }
figcaption { font-size: 13px; color: var(--muted); font-family: var(--mono); margin-top: 4px; }
details.logs pre { font: 12px/1.5 var(--mono); background: var(--surface); border: 1px solid var(--border);
  padding: 12px; overflow: auto; max-height: 480px; white-space: pre-wrap; overflow-wrap: anywhere; }
summary { cursor: pointer; color: var(--accent-text); }
form { margin: 0; }
label { display: block; font-weight: 600; margin-bottom: 4px; }
.hint { color: var(--muted); font-size: 13px; margin: 0 0 8px; }
textarea { width: 100%; min-height: 120px; font: inherit; color: var(--text); background: var(--surface);
  border: 1px solid var(--border-strong); border-radius: 2px; padding: 8px 12px; resize: vertical; }
select { font: inherit; color: var(--text); background: var(--surface); border: 1px solid var(--border-strong);
  border-radius: 2px; padding: 4px 8px; }
.row { display: flex; flex-wrap: wrap; align-items: center; gap: 12px; margin-top: 12px; }
.row label { margin: 0; }
button { font: inherit; font-weight: 600; border-radius: 2px; padding: 6px 16px; cursor: pointer;
  border: 1px solid var(--border-strong); background: var(--surface); color: var(--text); }
button.primary { background: var(--accent); border-color: var(--accent); color: var(--accent-on); }
button.danger { color: var(--danger); border-color: var(--danger); }
.actions { display: flex; flex-wrap: wrap; gap: 12px 24px; align-items: flex-start; margin-top: 32px;
  padding-top: 16px; border-top: 1px solid var(--border); }
.actions details[open] summary { margin-bottom: 8px; }
.notice { padding: 8px 12px; border-left: 2px solid var(--success); background: var(--surface); margin: 12px 0; }
@media (max-width: 560px) {
  .list a { grid-template-columns: minmax(0, 1fr); }
  .list .meta { grid-column: 1; grid-row: auto; text-align: left; }
  .tags { justify-content: flex-start; }
  dl.facts { grid-template-columns: minmax(0, 1fr); }
  dl.facts dd { margin-bottom: 8px; }
}
`

export function page(title: string, reader: string, body: string): string {
  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="robots" content="noindex">
<title>${escape(title)}</title>
<style>${STYLE}</style>
</head>
<body>
<div class="wrap">
<header><a class="name" href="/inbox">Ficelle feedback</a><span class="who">${escape(reader)}</span></header>
${body}
</div>
</body>
</html>`
}

export function when(ms: number): string {
  return `${new Date(ms).toISOString().slice(0, 16).replace('T', ' ')} UTC`
}

function size(bytes: number): string {
  return bytes < 1024 * 1024
    ? `${Math.max(1, Math.round(bytes / 1024))} KB`
    : `${(bytes / 1024 / 1024).toFixed(1)} MB`
}

export interface Counts {
  new: number
  open: number
  done: number
  spam: number
}

export interface Summary {
  id: string
  created_at: number
  status: Status
  category: string
  about: string | null
  excerpt: string
  version: string
  platform: string
  has_contact: number
  images: number
  answered: number
}

interface Tab {
  status: Status | undefined
  label: string
  empty: string
}

const INBOX: Tab = { status: undefined, label: 'Inbox', empty: 'No reports yet.' }

const TABS: Tab[] = [
  INBOX,
  { status: 'new', label: 'New', empty: 'Nothing new.' },
  { status: 'open', label: 'Open', empty: 'Nothing open.' },
  { status: 'done', label: 'Done', empty: 'Nothing done yet.' },
  { status: 'spam', label: 'Spam', empty: 'No spam.' },
]

function title(about: string | null, category: string): string {
  return (
    about ?? (category ? `${category.charAt(0).toUpperCase()}${category.slice(1)}` : 'No subject')
  )
}

function statusTag(status: Status): string {
  return `<span class="tag ${status}">${status}</span>`
}

export function listPage(
  reader: string,
  status: Status | undefined,
  counts: Counts,
  reports: Summary[],
  next: string | undefined,
): string {
  const count = (s: Status | undefined) =>
    s === undefined ? counts.new + counts.open + counts.done : counts[s]
  const tabs = TABS.map((t) => {
    const href = t.status ? `/inbox?status=${t.status}` : '/inbox'
    const current = t.status === status ? ' aria-current="page"' : ''
    return `<a href="${href}"${current}>${t.label}<span class="n">${count(t.status)}</span></a>`
  }).join('')
  const tab = TABS.find((t) => t.status === status) ?? INBOX
  const rows = reports
    .map((r) => {
      const tags = [
        r.images > 0 ? `<span class="tag">${r.images} img</span>` : '',
        r.has_contact ? '<span class="tag">contact</span>' : '',
        r.answered ? '<span class="tag">answered</span>' : '',
        statusTag(r.status),
      ].join('')
      const excerpt = r.excerpt.replace(/\s+/g, ' ').trim()
      return `<li><a href="/inbox/${r.id}">
<span class="about">${escape(title(r.about, r.category))}</span>
<span class="meta">${when(r.created_at)}<br>${escape(r.platform)} ${escape(r.version)}<span class="tags">${tags}</span></span>
<span class="excerpt">${excerpt ? escape(excerpt) : '&nbsp;'}</span>
</a></li>`
    })
    .join('\n')
  const more = next
    ? `<a class="more" href="/inbox?${new URLSearchParams({ ...(status ? { status } : {}), before: next })}">Older reports</a>`
    : ''
  const list = reports.length
    ? `<ul class="list">${rows}</ul>${more}`
    : `<p class="empty">${tab.empty}</p>`
  return page(
    `${tab.label} · Ficelle feedback`,
    reader,
    `<nav aria-label="Reports by status">${tabs}</nav>\n${list}`,
  )
}

export interface Full {
  id: string
  created_at: number
  status: Status
  category: string
  about: string | null
  message: string
  contact: string | null
  version: string
  platform: string
  logs: string | null
  install: string | null
  server: string | null
  score: number
  flags: string
  answer: string
  answerable: number
  from_install: number
}

export interface ImageRow {
  position: number
  kind: string
  width: number
  height: number
  size: number
}

export function reportPage(reader: string, r: Full, images: ImageRow[], saved: boolean): string {
  const contact = r.contact
    ? r.contact.includes('@') && !r.contact.startsWith('@')
      ? `<a href="mailto:${escape(r.contact)}">${escape(r.contact)}</a>`
      : `${escape(r.contact)} (Discord)`
    : 'none'
  const facts: [string, string][] = [
    ['Received', when(r.created_at)],
    ['App', `${escape(r.version)} on ${escape(r.platform)}`],
    ['Contact', contact],
    ['Server', r.server ? escape(r.server) : 'none'],
    [
      'Install',
      r.install
        ? `${escape(r.install.slice(0, 8))}… (${r.from_install} report${r.from_install === 1 ? '' : 's'})`
        : 'not sent',
    ],
    ['Spam score', `${r.score}${r.flags ? ` (${escape(r.flags.replaceAll(',', ', '))})` : ''}`],
    ['Status', statusTag(r.status)],
  ]
  const figures = images
    .map(
      (i) => `<figure>
<a href="/inbox/${r.id}/images/${i.position}"><img src="/inbox/${r.id}/images/${i.position}" width="${i.width}" height="${i.height}" loading="lazy" alt="Image ${i.position + 1} sent with the report"></a>
<figcaption>${escape(i.kind)} · ${i.width}×${i.height} · ${size(i.size)}</figcaption>
</figure>`,
    )
    .join('\n')
  const logLines = r.logs ? r.logs.split('\n').length : 0
  const statusOptions = (['new', 'open', 'done'] as const)
    .map(
      (s) =>
        `<option value="${s}"${r.status === s ? ' selected' : ''}>${s.charAt(0).toUpperCase()}${s.slice(1)}</option>`,
    )
    .join('')
  const answer = r.answerable
    ? `<label for="answer">Answer</label>
<p class="hint" id="answer-hint">Shown in the sender's app, under Sent feedback. 2,000 characters at most.</p>
<textarea id="answer" name="answer" maxlength="2000" aria-describedby="answer-hint">${escape(r.answer)}</textarea>`
    : `<p class="hint">This report came without a receipt, so the sender's app cannot show an answer.${r.contact ? ' Use the contact instead.' : ''}</p>`
  const spamButton =
    r.status === 'spam'
      ? '<button name="action" value="notSpam">Not spam</button>'
      : '<button name="action" value="spam">Mark as spam</button>'
  const body = `<a class="back" href="/inbox${r.status === 'spam' ? '?status=spam' : ''}">Back to the list</a>
<h1>${escape(title(r.about, r.category))}</h1>
${saved ? '<p class="notice" role="status">Saved.</p>' : ''}
<dl class="facts">${facts.map(([k, v]) => `<dt>${k}</dt><dd>${v}</dd>`).join('')}</dl>
<h2>Message</h2>
${r.message ? `<pre class="message">${escape(r.message)}</pre>` : '<p class="message none">No words beyond the subject.</p>'}
${images.length ? `<h2>Images</h2>\n<div class="images">${figures}</div>` : ''}
${r.logs ? `<h2>Logs</h2>\n<details class="logs"><summary>${logLines} line${logLines === 1 ? '' : 's'}</summary><pre>${escape(r.logs)}</pre></details>` : ''}
<h2>Answer and status</h2>
<form method="post" action="/inbox/${r.id}">
${answer}
<div class="row">
<label for="status">Status</label>
<select id="status" name="status"${r.status === 'spam' ? ' disabled' : ''}>${statusOptions}</select>
<button class="primary" name="action" value="save"${r.status === 'spam' ? ' disabled' : ''}>Save</button>
</div>
</form>
<div class="actions">
<form method="post" action="/inbox/${r.id}">${spamButton}</form>
<details><summary>Delete</summary>
<form method="post" action="/inbox/${r.id}"><p class="hint">The report and its images are gone for good, and the sender's app stops showing it.</p>
<button class="danger" name="action" value="delete">Delete this report</button></form>
</details>
</div>`
  return page(`${title(r.about, r.category)} · Ficelle feedback`, reader, body)
}

export function messagePage(reader: string, heading: string, text: string): string {
  return page(
    `${heading} · Ficelle feedback`,
    reader,
    `<h1>${escape(heading)}</h1><p>${escape(text)}</p><p><a href="/inbox">Back to the inbox</a></p>`,
  )
}
