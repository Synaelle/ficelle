<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- The feedback inbox: reports from anyone's apps, by status, with
     filters; one report whole with its images and logs, to mark, note, keep,
     block its sender or delete. Admins also set whether this server takes
     reports, where its people send theirs, who reads the inbox, the token
     for `reports pull` and the block lists. -->
<script lang="ts">
  import { t } from '../lib/i18n.svelte'
  import { overlay } from '../lib/overlay.svelte'
  import { api, download } from './api.svelte'
  import { bytes, dateTime, errorText } from './format'

  let { admin }: { admin: boolean } = $props()

  type Status = 'new' | 'open' | 'done' | 'spam'
  type SendTo = 'main' | 'here' | 'off'
  type BlockKind = 'install' | 'address' | 'email' | 'word'
  interface Summary {
    id: string
    createdAt: number
    status: Status
    /** Empty from apps since 0.2.0, they send `about`. */
    category: string
    about: string | null
    excerpt: string
    version: string
    platform: string
    hasContact: boolean
    hasLogs: boolean
    images: number
    score: number
    flags: string
    kept: boolean
  }
  interface Page {
    reports: Summary[]
    next?: string
    counts: Record<Status, number>
    versions: string[]
    platforms: string[]
  }
  interface Full {
    id: string
    createdAt: number
    status: Status
    category: string
    about: string | null
    message: string
    /** An email address or a Discord name. */
    contact: string | null
    version: string
    platform: string
    logs: string | null
    install: string | null
    score: number
    flags: string
    note: string
    /** The one answer the sender's app shows. */
    answer: string
    /** Sent with a receipt: the sender's app can read the answer. */
    answerable: boolean
    kept: boolean
    fromSender: number
    images: { position: number; kind: string; width: number; height: number; size: number }[]
  }
  interface Block {
    kind: BlockKind
    value: string
    note: string
    createdAt: number
    expiresAt: number | null
  }
  interface Settings {
    receive: boolean
    sendTo: SendTo
    readers: { id: string; username: string; addedAt: number }[]
    tokenCreatedAt: number | null
    /** Set on this page, null for the config's. */
    caps: Caps
    /** The config's, what an empty field means. */
    defaultCaps: Caps
  }
  interface Caps {
    perHour: number | null
    perDay: number | null
  }

  const STATUSES: Status[] = ['new', 'open', 'done', 'spam']

  let status: Status = $state('new')
  let platform = $state('')
  let version = $state('')
  let page: Page | null = $state(null)
  let reports: Summary[] = $state([])
  let open: Full | null = $state(null)
  let images: string[] = $state([])
  let note = $state('')
  let answer = $state('')
  let problem = $state('')
  let saved = $state('')
  let busy = $state(false)
  let settings: Settings | null = $state(null)
  let blocks: Block[] = $state([])
  let reader = $state('')
  let token = $state('')
  let blockKind: BlockKind = $state('word')
  let blockValue = $state('')

  async function act(work: () => Promise<void>) {
    busy = true
    problem = ''
    saved = ''
    try {
      await work()
    } catch (err) {
      problem = errorText(err)
    } finally {
      busy = false
    }
  }

  async function load(more = false) {
    await act(async () => {
      const params = new URLSearchParams({
        status,
        ...(platform ? { platform } : {}),
        ...(version ? { version } : {}),
        ...(more && page?.next ? { before: page.next } : {}),
      })
      const next = await api<Page>('GET', `/admin/feedback/reports?${params}`)
      reports = more ? [...reports, ...next.reports] : next.reports
      page = next
    })
  }

  function forget() {
    for (const url of images) URL.revokeObjectURL(url)
    images = []
  }

  function show(full: Full) {
    open = full
    note = full.note
    answer = full.answer
  }

  async function read(id: string) {
    await act(async () => {
      forget()
      const full = await api<Full>('GET', `/admin/feedback/reports/${id}`)
      show(full)
      // fetched with the session and shown from memory: the server only
      // hands them out as downloads
      images = await Promise.all(
        full.images.map(async (i) =>
          URL.createObjectURL(await download(`/admin/feedback/reports/${id}/images/${i.position}`)),
        ),
      )
    })
  }

  function back() {
    forget()
    open = null
    void load()
  }

  async function change(body: { status?: Status; note?: string; answer?: string; kept?: boolean }) {
    const id = open?.id
    if (!id) return
    await act(async () => {
      show(await api<Full>('PATCH', `/admin/feedback/reports/${id}`, body))
      saved = t('admin.feedback.saved')
    })
  }

  async function blockSender(kind: BlockKind) {
    const id = open?.id
    if (!id) return
    const yes = await overlay.ask(t(`admin.feedback.blockConfirm_${kind}`), {
      confirmLabel: t('admin.feedback.block'),
      danger: true,
    })
    if (!yes) return
    // addresses change hands, their blocks end
    const days = kind === 'address' ? 30 : undefined
    await act(async () => {
      show(await api<Full>('POST', `/admin/feedback/reports/${id}/block`, { kind, days }))
      saved = t('admin.feedback.blocked')
      if (blocks.length || admin) await loadBlocks()
    })
  }

  async function remove() {
    const id = open?.id
    if (!id) return
    const yes = await overlay.ask(t('admin.feedback.deleteConfirm'), {
      confirmLabel: t('admin.feedback.delete'),
      danger: true,
    })
    if (!yes) return
    await act(async () => {
      await api('DELETE', `/admin/feedback/reports/${id}`)
      back()
    })
  }

  async function loadBlocks() {
    blocks = await api<Block[]>('GET', '/admin/feedback/blocks')
  }

  async function loadSettings() {
    if (!admin) return
    settings = await api<Settings>('GET', '/admin/feedback/settings')
  }

  async function setSwitches(next: { receive: boolean; sendTo: SendTo }) {
    await act(async () => {
      settings = await api<Settings>('PUT', '/admin/feedback/settings', next)
      saved = t('admin.feedback.saved')
    })
  }

  /** The caps as typed, an empty field for the config's. */
  let capsDraft = $state({ perHour: '', perDay: '' })
  $effect(() => {
    if (!settings) return
    capsDraft = {
      perHour: settings.caps.perHour?.toString() ?? '',
      perDay: settings.caps.perDay?.toString() ?? '',
    }
  })

  async function saveCaps(event: SubmitEvent) {
    event.preventDefault()
    const cap = (text: string) => (text.trim() ? Number(text) : null)
    await act(async () => {
      settings = await api<Settings>('PUT', '/admin/feedback/caps', {
        perHour: cap(capsDraft.perHour),
        perDay: cap(capsDraft.perDay),
      })
      saved = t('admin.feedback.saved')
    })
  }

  async function addReader(event: SubmitEvent) {
    event.preventDefault()
    const name = reader.trim()
    if (!name) return
    await act(async () => {
      settings = await api<Settings>('PUT', `/admin/feedback/readers/${encodeURIComponent(name)}`)
      reader = ''
    })
  }

  async function removeReader(id: string) {
    await act(async () => {
      settings = await api<Settings>('DELETE', `/admin/feedback/readers/${id}`)
    })
  }

  async function newToken() {
    if (settings?.tokenCreatedAt) {
      const yes = await overlay.ask(t('admin.feedback.tokenReplace'), {
        confirmLabel: t('admin.feedback.tokenNew'),
      })
      if (!yes) return
    }
    await act(async () => {
      token = (await api<{ token: string }>('POST', '/admin/feedback/token')).token
      await loadSettings()
    })
  }

  async function revokeToken() {
    await act(async () => {
      await api('DELETE', '/admin/feedback/token')
      token = ''
      await loadSettings()
    })
  }

  async function addBlock(event: SubmitEvent) {
    event.preventDefault()
    const value = blockValue.trim()
    if (!value) return
    await act(async () => {
      blocks = await api<Block[]>('POST', '/admin/feedback/blocks', { kind: blockKind, value })
      blockValue = ''
    })
  }

  async function unblock(block: Block) {
    await act(async () => {
      const params = new URLSearchParams({ kind: block.kind, value: block.value })
      blocks = await api<Block[]>('DELETE', `/admin/feedback/blocks?${params}`)
    })
  }

  function flagText(flags: string) {
    return flags
      .split(',')
      .filter(Boolean)
      .map((f) => t(`admin.feedback.flag_${f}`))
      .join(', ')
  }

  void load()
  void act(async () => {
    await loadSettings()
    await loadBlocks()
  })
</script>

{#if problem}<p class="problem" role="alert">{problem}</p>{/if}
{#if saved}<p class="saved" role="status">{saved}</p>{/if}

{#if open}
  {@const report = open}
  <section class="report" aria-busy={busy}>
    <button class="btn" onclick={back}>{t('admin.feedback.back')}</button>
    <p class="meta muted">
      {dateTime(report.createdAt)} ·
      {#if report.category}{t(`admin.feedback.category_${report.category}`)} ·{/if}
      {report.platform} · {report.version} · {t(`admin.feedback.status_${report.status}`)}
      {#if report.fromSender > 1}· {t('admin.feedback.fromSender', {
          count: report.fromSender,
        })}{/if}
    </p>
    {#if report.flags}
      <p class="flags">
        {t('admin.feedback.score', { score: report.score, flags: flagText(report.flags) })}
      </p>
    {/if}
    {#if report.about}<h3 class="about">{report.about}</h3>{/if}
    {#if report.message}<p class="message">{report.message}</p>{/if}
    {#if report.contact}
      <p>
        {t('admin.feedback.contact')}
        {#if report.contact.includes('@')}
          <a href={`mailto:${report.contact}`}>{report.contact}</a>
        {:else}
          {t('admin.feedback.discord', { name: report.contact })}
        {/if}
      </p>
    {/if}
    {#each report.images as image, n (image.position)}
      <figure>
        {#if images[n]}
          <a href={images[n]} download={`${report.id}-${image.position}.${image.kind}`}>
            <img src={images[n]} alt={t('admin.feedback.image', { n: n + 1 })} />
          </a>
        {/if}
        <figcaption class="muted">
          {image.width} × {image.height} · {bytes(image.size)}
        </figcaption>
      </figure>
    {/each}
    {#if report.logs}
      <details>
        <summary>{t('admin.feedback.logs')}</summary>
        <pre>{report.logs}</pre>
      </details>
    {/if}

    <div class="actions">
      {#each STATUSES as s (s)}
        {#if s !== report.status}
          <button class="btn" disabled={busy} onclick={() => change({ status: s })}
            >{t(`admin.feedback.mark_${s}`)}</button
          >
        {/if}
      {/each}
      <label class="choice">
        <input
          type="checkbox"
          checked={report.kept}
          disabled={busy}
          onchange={(e) => change({ kept: e.currentTarget.checked })}
        />
        {t('admin.feedback.keep')}
      </label>
    </div>
    <label class="field-label">
      {t('admin.feedback.note')}
      <textarea class="field" rows="3" maxlength="2000" bind:value={note}></textarea>
    </label>
    <button class="btn" disabled={busy || note === report.note} onclick={() => change({ note })}
      >{t('admin.feedback.saveNote')}</button
    >
    {#if report.answerable}
      <label class="field-label">
        {t('admin.feedback.answer')}
        <textarea class="field" rows="3" maxlength="2000" bind:value={answer} data-feedback-answer
        ></textarea>
        <span class="muted small">{t('admin.feedback.answerHelp')}</span>
      </label>
      <button
        class="btn"
        disabled={busy || answer === report.answer}
        onclick={() => change({ answer })}>{t('admin.feedback.saveAnswer')}</button
      >
    {/if}
    <div class="actions">
      {#if report.install}
        <button class="btn" disabled={busy} onclick={() => blockSender('install')}
          >{t('admin.feedback.block_install')}</button
        >
      {/if}
      <button class="btn" disabled={busy} onclick={() => blockSender('address')}
        >{t('admin.feedback.block_address')}</button
      >
      {#if report.contact}
        <button class="btn" disabled={busy} onclick={() => blockSender('email')}
          >{t('admin.feedback.block_contact')}</button
        >
      {/if}
      <button class="btn danger" disabled={busy} onclick={remove}
        >{t('admin.feedback.delete')}</button
      >
    </div>
  </section>
{:else}
  <section aria-busy={busy}>
    <div class="tabs" role="tablist">
      {#each STATUSES as s (s)}
        <button
          role="tab"
          aria-selected={status === s}
          onclick={() => {
            status = s
            void load()
          }}
          >{t(`admin.feedback.status_${s}`)}
          <span class="count">{page?.counts[s] ?? 0}</span></button
        >
      {/each}
    </div>
    <div class="actions">
      <label class="field-label small-field">
        {t('admin.feedback.platform')}
        <select class="field" bind:value={platform} onchange={() => load()}>
          <option value="">{t('admin.feedback.any')}</option>
          {#each page?.platforms ?? [] as p (p)}<option value={p}>{p}</option>{/each}
        </select>
      </label>
      <label class="field-label small-field">
        {t('admin.feedback.version')}
        <select class="field" bind:value={version} onchange={() => load()}>
          <option value="">{t('admin.feedback.any')}</option>
          {#each page?.versions ?? [] as v (v)}<option value={v}>{v}</option>{/each}
        </select>
      </label>
    </div>
    <ul class="reports">
      {#each reports as report (report.id)}
        <li>
          <button onclick={() => read(report.id)}>
            <span class="meta muted">
              {dateTime(report.createdAt)} ·
              {#if report.category}{t(`admin.feedback.category_${report.category}`)} ·{/if}
              {report.platform} · {report.version}
              {#if report.images}· {t('admin.feedback.images', { count: report.images })}{/if}
              {#if report.hasLogs}· {t('admin.feedback.logs')}{/if}
              {#if report.hasContact}· {t('admin.feedback.hasContact')}{/if}
              {#if report.kept}· {t('admin.feedback.kept')}{/if}
            </span>
            {#if report.about}<strong class="excerpt">{report.about}</strong>{/if}
            {#if report.excerpt}<span class="excerpt">{report.excerpt}</span>{/if}
          </button>
        </li>
      {:else}
        <li class="muted">{t('admin.feedback.none')}</li>
      {/each}
    </ul>
    {#if page?.next}
      <button class="btn" onclick={() => load(true)}>{t('admin.feedback.older')}</button>
    {/if}
  </section>

  {#if settings}
    <section>
      <h2>{t('admin.feedback.settings')}</h2>
      <label class="choice">
        <input
          type="checkbox"
          checked={settings.receive}
          disabled={busy}
          onchange={(e) =>
            setSwitches({ receive: e.currentTarget.checked, sendTo: settings?.sendTo ?? 'main' })}
        />
        {t('admin.feedback.receive')}
      </label>
      <p class="muted small">{t('admin.feedback.receiveHelp')}</p>
      {#if settings.receive}
        <form class="caps" onsubmit={saveCaps}>
          <p class="muted small">{t('admin.feedback.capsHelp')}</p>
          <label
            >{t('admin.feedback.perHour')}<input
              class="field"
              type="number"
              min="1"
              max="100000"
              placeholder={String(settings.defaultCaps.perHour ?? '')}
              bind:value={capsDraft.perHour}
            /></label
          >
          <label
            >{t('admin.feedback.perDay')}<input
              class="field"
              type="number"
              min="1"
              max="100000"
              placeholder={String(settings.defaultCaps.perDay ?? '')}
              bind:value={capsDraft.perDay}
            /></label
          >
          <button class="btn" type="submit" disabled={busy}>{t('admin.feedback.saveCaps')}</button>
        </form>
      {/if}
      <fieldset>
        <legend>{t('admin.feedback.sendTo')}</legend>
        {#each ['main', 'here', 'off'] as const as to (to)}
          <label class="choice">
            <input
              type="radio"
              name="send-to"
              checked={settings.sendTo === to}
              disabled={busy || (to === 'here' && !settings.receive)}
              onchange={() => setSwitches({ receive: settings?.receive ?? false, sendTo: to })}
            />
            {t(`admin.feedback.sendTo_${to}`)}
          </label>
        {/each}
      </fieldset>
    </section>

    <section>
      <h2>{t('admin.feedback.readers')}</h2>
      <p class="muted small">{t('admin.feedback.readersHelp')}</p>
      <ul class="plain">
        {#each settings.readers as person (person.id)}
          <li>
            {person.username}
            <button class="btn" disabled={busy} onclick={() => removeReader(person.id)}
              >{t('admin.feedback.remove')}</button
            >
          </li>
        {:else}
          <li class="muted">{t('admin.feedback.noReaders')}</li>
        {/each}
      </ul>
      <form class="actions" onsubmit={addReader}>
        <label class="field-label small-field">
          {t('admin.feedback.username')}
          <input class="field" bind:value={reader} autocomplete="off" />
        </label>
        <button class="btn" disabled={busy || !reader.trim()}
          >{t('admin.feedback.addReader')}</button
        >
      </form>
    </section>

    <section>
      <h2>{t('admin.feedback.token')}</h2>
      <p class="muted small">{t('admin.feedback.tokenHelp')}</p>
      {#if token}
        <p class="note">{t('admin.feedback.tokenOnce')}</p>
        <code class="token">{token}</code>
      {:else if settings.tokenCreatedAt}
        <p>{t('admin.feedback.tokenMade', { when: dateTime(settings.tokenCreatedAt) })}</p>
      {/if}
      <div class="actions">
        <button class="btn" disabled={busy} onclick={newToken}
          >{t('admin.feedback.tokenNew')}</button
        >
        {#if settings.tokenCreatedAt}
          <button class="btn danger" disabled={busy} onclick={revokeToken}
            >{t('admin.feedback.tokenRevoke')}</button
          >
        {/if}
      </div>
    </section>
  {/if}

  <section>
    <h2>{t('admin.feedback.blocks')}</h2>
    <p class="muted small">{t('admin.feedback.blocksHelp')}</p>
    <ul class="plain">
      {#each blocks as block (`${block.kind}:${block.value}`)}
        <li>
          <span>
            {t(`admin.feedback.blockKind_${block.kind}`)}:
            <span class={block.kind === 'address' || block.kind === 'install' ? 'mono' : ''}
              >{block.value}</span
            >
            <span class="muted small">
              {block.expiresAt
                ? t('admin.feedback.until', { when: dateTime(block.expiresAt) })
                : t('admin.feedback.forever')}
            </span>
          </span>
          <button class="btn" disabled={busy} onclick={() => unblock(block)}
            >{t('admin.feedback.unblock')}</button
          >
        </li>
      {:else}
        <li class="muted">{t('admin.feedback.noBlocks')}</li>
      {/each}
    </ul>
    <form class="actions" onsubmit={addBlock}>
      <label class="field-label small-field">
        {t('admin.feedback.blockWhat')}
        <select class="field" bind:value={blockKind}>
          <option value="word">{t('admin.feedback.blockKind_word')}</option>
          <option value="email">{t('admin.feedback.blockKind_contact')}</option>
        </select>
      </label>
      <label class="field-label small-field">
        {t('admin.feedback.blockValue')}
        <input class="field" bind:value={blockValue} maxlength="254" autocomplete="off" />
      </label>
      <button class="btn" disabled={busy || !blockValue.trim()}>{t('admin.feedback.block')}</button>
    </form>
  </section>
{/if}

<style>
  section,
  form {
    display: grid;
    gap: var(--space-3);
    margin-bottom: var(--space-6);
    justify-items: start;
  }
  form {
    margin-bottom: 0;
  }
  h2 {
    margin: 0;
    font-size: 16px;
  }
  p {
    margin: 0;
  }
  .small {
    font-size: 12px;
  }
  .note {
    font-weight: 600;
  }
  .problem {
    color: var(--color-danger);
  }
  .saved {
    margin-bottom: var(--space-3);
  }
  .caps {
    display: flex;
    flex-wrap: wrap;
    align-items: end;
    gap: var(--space-3);
    margin: var(--space-3) 0;
  }
  .caps p {
    flex-basis: 100%;
    margin: 0;
  }
  .caps label {
    display: grid;
    gap: var(--space-1);
  }
  .caps .field {
    width: 9em;
  }
  fieldset {
    display: grid;
    gap: var(--space-1);
    margin: 0;
    padding: 0;
    border: 0;
  }
  legend {
    margin-bottom: var(--space-1);
    font-weight: 600;
  }
  .choice {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .field-label {
    display: grid;
    gap: var(--space-1);
    width: min(520px, 100%);
    font-weight: 600;
  }
  .small-field {
    width: auto;
    min-width: 10em;
  }
  .field-label .field {
    font-weight: 400;
  }
  textarea {
    height: auto;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    align-items: end;
    gap: var(--space-3);
  }
  .tabs {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1);
  }
  .tabs button {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    height: 32px;
    padding: 0 var(--space-3);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-control);
    background: none;
  }
  .tabs button[aria-selected='true'] {
    background: var(--color-selection);
  }
  .count {
    color: var(--color-text-muted);
    font-size: 12px;
  }
  .reports,
  .plain {
    justify-self: stretch;
    display: grid;
    gap: var(--space-2);
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .reports button {
    display: grid;
    gap: var(--space-1);
    width: 100%;
    padding: var(--space-2) var(--space-3);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-popover);
    background: var(--color-surface);
    text-align: left;
  }
  .excerpt {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .meta {
    font-size: 12px;
  }
  .plain li {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    padding: var(--space-1) 0;
    border-bottom: 1px solid var(--color-border);
  }
  .report {
    max-width: 760px;
  }
  .about {
    margin: 0;
    font-size: 16px;
    overflow-wrap: anywhere;
  }
  .message {
    justify-self: stretch;
    padding: var(--space-3);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-popover);
    background: var(--color-surface);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .flags {
    color: var(--color-danger);
    font-size: 13px;
  }
  figure {
    display: grid;
    gap: var(--space-1);
    margin: 0;
  }
  img {
    max-width: min(100%, 640px);
    max-height: 480px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-control);
  }
  details {
    justify-self: stretch;
  }
  pre {
    max-height: 320px;
    overflow: auto;
    padding: var(--space-2);
    background: var(--color-surface);
    font-family: var(--font-mono);
    font-size: 12px;
    white-space: pre-wrap;
  }
  .mono,
  .token {
    font-family: var(--font-mono);
    font-size: 12px;
    overflow-wrap: anywhere;
  }
  .token {
    padding: var(--space-2);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-control);
    user-select: all;
  }
</style>
