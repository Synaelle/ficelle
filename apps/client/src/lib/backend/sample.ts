// SPDX-License-Identifier: AGPL-3.0-or-later
// The design sample notes, for the browser preview. Same content as the
// devtool seed (crates/devtool/src/seed.rs).

import type { Folder, Note } from './types'

const DAY = 86_400_000

export const TRELLIS = `Six cedar posts along the south bed, jute twine strung between them. Same setup as last year but tighter spacing. See [[Garden log 2025]] for what went wrong.

## This weekend

- [x] Buy jute twine (2 rolls)
- [x] Sharpen the pruners
- [ ] Set the posts 60 cm deep
- [ ] Ask Inès for the ladder

> [!note]
> Prune suckers every week once the plants pass 30 cm.

\`\`\`
spacing_cm = 45
rows = 2
posts = 6
\`\`\`
`

export function sampleData(now: number): { folders: Folder[]; notes: Note[] } {
  const folder = (id: string, name: string, parentId: string | null, position: number): Folder => ({
    id,
    parentId,
    name,
    position,
    createdAt: now - 30 * DAY,
    updatedAt: now - 30 * DAY,
    trashedAt: null,
    spaceId: '',
  })
  const folders = [
    folder('f-inbox', 'Inbox', null, 0),
    folder('f-journal', 'Journal', null, 1),
    folder('f-projects', 'Projects', null, 2),
    folder('f-garden', 'Garden', 'f-projects', 0),
    folder('f-kitchen', 'Kitchen', 'f-projects', 1),
    folder('f-reading', 'Reading', null, 3),
  ]
  let age = 12
  const note = (
    folderId: string,
    title: string,
    body: string,
    tags: string[],
    pinned = false,
  ): Note => {
    age -= 2
    return {
      id: `n-${age}`,
      folderId,
      title,
      body,
      pinned,
      tags,
      createdAt: now - age * DAY,
      updatedAt: now - age * DAY,
      trashedAt: null,
      spaceId: '',
    }
  }
  const notes = [
    note('f-reading', 'Reading list, autumn', 'The Overstory, Piranesi, La Horde du Contrevent.', [
      'ideas',
    ]),
    note(
      'f-inbox',
      'Ideas for the shed',
      'Pegboard wall, a shelf for seed trays, hooks for the long tools.',
      ['ideas'],
    ),
    note(
      'f-journal',
      'Sync design scratch',
      'What happens when a folder moves twice offline? Both moves stay in history.',
      ['work'],
    ),
    note(
      'f-inbox',
      'Call with Inès',
      'She can lend the long ladder on Saturday. Ask about the [[Tomato trellis plan]].',
      ['todo'],
    ),
    note(
      'f-kitchen',
      'Pain au levain',
      'Levain at 100% hydration, 500 g flour, 350 g water, 10 g salt.',
      ['recettes'],
      true,
    ),
    note('f-garden', 'Tomato trellis plan', TRELLIS, ['todo', 'garden'], true),
  ]
  return { folders, notes }
}
