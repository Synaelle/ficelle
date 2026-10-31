// SPDX-License-Identifier: AGPL-3.0-or-later
// Turns markdown files from other apps into notes. Notes keep their title
// apart from the body, so a leading front matter title or level one heading
// becomes the title instead of staying in the text twice.

export interface ImportedNote {
  title: string
  body: string
  tags: string[]
}

const MARKDOWN_FILE = /\.(md|markdown|mdown|mkd)$/i

export function isMarkdownFile(file: { name: string }): boolean {
  return MARKDOWN_FILE.test(file.name)
}

// Obsidian and static site generators put metadata in a yaml block at the top
const FRONT_MATTER = /^---\n([\s\S]*?)\n---[ \t]*(?:\n|$)/
const LEADING_HEADING = /^[ \t]*#[ \t]+(.+?)[ \t#]*(?:\n|$)/

const unquote = (value: string) => value.trim().replace(/^(['"])(.*)\1$/, '$2')

/** Reads `title` and `tags` from front matter, only the common simple forms. */
function readFrontMatter(yaml: string): { title?: string; tags: string[] } {
  const lines = yaml.split('\n')
  let title: string | undefined
  const tags: string[] = []
  lines.forEach((line, i) => {
    const field = /^(title|tags):(.*)$/.exec(line)
    if (!field) return
    const [, key, raw = ''] = field
    if (key === 'title') {
      title = unquote(raw) || undefined
      return
    }
    const inline = raw.trim().replace(/^\[(.*)\]$/, '$1')
    if (inline) {
      tags.push(...inline.split(',').map(unquote))
      return
    }
    // block list: following lines that start with a dash
    for (const item of lines.slice(i + 1)) {
      const entry = /^\s*-\s+(.*)$/.exec(item)
      if (!entry) break
      tags.push(unquote(entry[1] ?? ''))
    }
  })
  return { title, tags: tags.map((tag) => tag.replace(/^#/, '')).filter(Boolean) }
}

export function parseMarkdownFile(fileName: string, text: string): ImportedNote {
  let body = text.replace(/^\uFEFF/, '').replace(/\r\n?/g, '\n')
  let title: string | undefined
  let tags: string[] = []

  const front = FRONT_MATTER.exec(body)
  if (front) {
    const meta = readFrontMatter(front[1] ?? '')
    title = meta.title
    tags = meta.tags
    body = body.slice(front[0].length)
  }
  if (!title) {
    const heading = LEADING_HEADING.exec(body.replace(/^\n+/, ''))
    if (heading) {
      title = heading[1]
      body = body.replace(/^\n+/, '').slice(heading[0].length)
    }
  }
  title ??= fileName.replace(MARKDOWN_FILE, '')
  return { title: title.trim(), body: body.replace(/^\n+/, ''), tags }
}
