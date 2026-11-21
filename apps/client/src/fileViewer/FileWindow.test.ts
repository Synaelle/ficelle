// SPDX-License-Identifier: AGPL-3.0-or-later
// A file opened with the app reaches nothing of the notes until it is
// imported: every backend call the file window makes is recorded.

import { flushSync, mount, unmount } from 'svelte'
import { afterEach, describe, expect, it, vi } from 'vitest'

const called: string[] = []
const unlocked = { unlocked: true, exists: true, way: 'plain' }

vi.mock('../lib/backend', () => ({
  backend: new Proxy(
    {},
    {
      get: (_, name: string) => {
        if (name === 'native') return true
        if (name === 'then') return undefined
        return (...args: unknown[]) => {
          called.push(name)
          if (name === 'vaultStatus') return Promise.resolve(unlocked)
          if (name === 'createNote')
            return Promise.resolve({ id: 'n1', ...(args[0] as object), updatedAt: 0 })
          return Promise.resolve(null)
        }
      },
    },
  ),
}))

const written: string[] = []
vi.mock('./files', () => ({
  readOpenedFile: () =>
    Promise.resolve({ name: 'todo.txt', text: 'buy *milk*\n', plain: true, stamp: '1' }),
  writeOpenedFile: (text: string) => {
    written.push(text)
    return Promise.resolve('2')
  },
  showInMain: () => Promise.resolve(),
}))
vi.mock('../lib/windows', () => ({
  announceUnlocked: () => Promise.resolve(),
  windowReady: () => Promise.resolve(),
}))

const { default: FileWindow } = await import('./FileWindow.svelte')
const settle = () => new Promise((resolve) => setTimeout(resolve, 0))

let shown: ReturnType<typeof mount> | null = null
afterEach(() => {
  if (shown) void unmount(shown)
  shown = null
  document.body.innerHTML = ''
  called.length = 0
  written.length = 0
})

describe('FileWindow', () => {
  it('calls nothing of the notes before the import, then creates one note', async () => {
    vi.useFakeTimers()
    shown = mount(FileWindow, { target: document.body, props: { close: () => {} } })
    await vi.runAllTimersAsync()
    flushSync()
    const area = document.querySelector('textarea') as HTMLTextAreaElement
    expect(area.value).toBe('buy *milk*\n')
    area.value = 'buy *milk* and bread\n'
    area.dispatchEvent(new Event('input', { bubbles: true }))
    await vi.advanceTimersByTimeAsync(1500)
    expect(written).toEqual(['buy *milk* and bread\n'])
    expect(called).toEqual([])

    const button = [...document.querySelectorAll('button')].find((b) =>
      b.textContent?.includes('Import'),
    ) as HTMLButtonElement
    button.click()
    await vi.runAllTimersAsync()
    vi.useRealTimers()
    await settle()
    expect(called).toEqual(['vaultStatus', 'createNote'])
  })
})
