// SPDX-License-Identifier: AGPL-3.0-or-later

import { applyD1Migrations, env } from 'cloudflare:test'
import { beforeEach } from 'vitest'

await applyD1Migrations(env.DB, env.MIGRATIONS)

// storage lasts the whole file: each test starts from no reports and no limits
beforeEach(async () => {
  await env.DB.batch(
    ['images', 'reports', 'sends', 'used_challenges', 'asks'].map((table) =>
      env.DB.prepare(`DELETE FROM ${table}`),
    ),
  )
  const stored = await env.IMAGES.list()
  if (stored.objects.length > 0) await env.IMAGES.delete(stored.objects.map((o) => o.key))
})
