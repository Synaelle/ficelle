// SPDX-License-Identifier: AGPL-3.0-or-later
// The design sample notes, for the browser preview, written through the
// core when it starts. Same content as the devtool seed
// (crates/devtool/src/seed.rs).

import type { NewNote } from './types'

const TRELLIS = `Six cedar posts along the south bed, jute twine strung between them. Same setup as last year but tighter spacing. See [[Garden log 2025]] for what went wrong.

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

/** The sample notes, oldest first. */
export function sampleNotes(): NewNote[] {
  const note = (
    place: string | null,
    title: string,
    body: string,
    tags: string[],
    pinned = false,
  ): NewNote => ({ title, body, pinned, tags: place ? [place, ...tags] : tags })
  return [
    note('Reading', 'Reading list, autumn', 'The Overstory, Piranesi, La Horde du Contrevent.', [
      'ideas',
    ]),
    note(
      null,
      'Ideas for the shed',
      'Pegboard wall, a shelf for seed trays, hooks for the long tools.',
      ['ideas'],
    ),
    note(
      'Journal',
      'Sync design scratch',
      'What happens when a folder moves twice offline? Both moves stay in history.',
      ['work'],
    ),
    note(
      null,
      'Call with Inès',
      'She can lend the long ladder on Saturday. Ask about the [[Tomato trellis plan]].',
      ['todo'],
    ),
    note(
      'Projects/Kitchen',
      'Pain au levain',
      'Levain at 100% hydration, 500 g flour, 350 g water, 10 g salt.',
      ['recettes'],
      true,
    ),
    note('Projects/Garden', 'Tomato trellis plan', TRELLIS, ['todo'], true),
  ]
}
