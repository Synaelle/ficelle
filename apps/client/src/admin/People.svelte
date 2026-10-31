<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- People: search, add, and everything an admin does to one account. -->
<script lang="ts">
  import PasswordField from '../lib/PasswordField.svelte'
  import { t } from '../lib/i18n.svelte'
  import Icon from '../lib/icons/Icon.svelte'
  import { overlay } from '../lib/overlay.svelte'
  import { api, auth, type AdminUser, type Device, type Role } from './api.svelte'
  import { dateTime, errorText, relative } from './format'
  import Policy from './Policy.svelte'

  const PAGE = 50
  let query = $state('')
  let offset = $state(0)
  let total = $state(0)
  let people: AdminUser[] = $state([])
  let problem = $state('')
  let notice = $state('')

  let open: { user: AdminUser; deviceList: Device[] } | null = $state(null)

  let adding = $state(false)
  let draft = $state({ username: '', displayName: '', password: '', role: 'user' as Role })

  /** Runs a call. `done` is the confirmation to show, the reloads that
   * follow an action pass none and leave it on screen. */
  async function run(action: () => Promise<unknown>, done = '') {
    problem = ''
    if (done) notice = ''
    try {
      await action()
      if (done) notice = done
      return true
    } catch (err) {
      problem = errorText(err)
      return false
    }
  }

  async function load() {
    const params = new URLSearchParams({ query, offset: String(offset), limit: String(PAGE) })
    await run(async () => {
      const page = await api<{ total: number; items: AdminUser[] }>('GET', `/admin/users?${params}`)
      total = page.total
      people = page.items
    })
  }

  let timer: ReturnType<typeof setTimeout> | undefined
  function search() {
    clearTimeout(timer)
    timer = setTimeout(() => {
      offset = 0
      void load()
    }, 250)
  }

  async function show(id: string) {
    await run(async () => {
      open = await api('GET', `/admin/users/${id}`)
    })
  }

  async function refresh() {
    if (open) await show(open.user.id)
    await load()
  }

  async function add(event: SubmitEvent) {
    event.preventDefault()
    const ok = await run(
      () => api('POST', '/admin/users', draft),
      t('admin.people.added', { name: draft.username }),
    )
    if (ok) {
      adding = false
      draft = { username: '', displayName: '', password: '', role: 'user' }
      await load()
    }
  }

  async function patch(change: Record<string, unknown>, done: string) {
    if (!open) return
    const id = open.user.id
    if (await run(() => api('PATCH', `/admin/users/${id}`, change), done)) await refresh()
  }

  async function rename() {
    if (!open) return
    const name = await overlay.askText(t('admin.people.renamePrompt'), open.user.displayName)
    if (name?.trim()) await patch({ displayName: name }, t('admin.people.saved'))
  }

  async function toggleOff() {
    if (!open) return
    const off = !open.user.disabledAt
    if (off) {
      const yes = await overlay.ask(
        t('admin.people.disableConfirm', { name: open.user.username }),
        {
          confirmLabel: t('admin.people.disable'),
          danger: true,
        },
      )
      if (!yes) return
    }
    await patch({ disabled: off }, t(off ? 'admin.people.disabled' : 'admin.people.enabled'))
  }

  async function resetPassword() {
    if (!open) return
    const id = open.user.id
    const password = await overlay.askPassword(
      t('admin.people.passwordPrompt', { name: open.user.username }),
    )
    if (password)
      await run(
        () => api('POST', `/admin/users/${id}/password`, { password }),
        t('admin.people.passwordDone'),
      )
    await refresh()
  }

  async function signOutEverywhere() {
    if (!open) return
    const id = open.user.id
    await run(() => api('POST', `/admin/users/${id}/signout`), t('admin.people.signedOut'))
    await refresh()
  }

  async function resetFactors() {
    if (!open) return
    const { id, username } = open.user
    const yes = await overlay.ask(t('admin.people.resetFactorsConfirm', { name: username }), {
      confirmLabel: t('admin.people.resetFactors'),
      danger: true,
    })
    if (!yes) return
    await run(
      () => api('POST', `/admin/users/${id}/second-factors/reset`),
      t('admin.people.factorsReset'),
    )
    await refresh()
  }

  async function remove() {
    if (!open) return
    const { id, username } = open.user
    const yes = await overlay.ask(t('admin.people.deleteConfirm', { name: username }), {
      confirmLabel: t('admin.people.delete'),
      danger: true,
    })
    if (!yes) return
    if (
      await run(
        () => api('DELETE', `/admin/users/${id}`),
        t('admin.people.deleted', { name: username }),
      )
    ) {
      open = null
      await load()
    }
  }

  void load()
</script>

<div class="bar">
  <input
    class="field"
    type="search"
    placeholder={t('admin.people.search')}
    aria-label={t('admin.people.search')}
    bind:value={query}
    oninput={search}
  />
  <button class="btn primary" onclick={() => (adding = !adding)}>
    <Icon name="plus" size={16} />{t('admin.people.add')}
  </button>
</div>

{#if problem}<p class="problem" role="alert">{problem}</p>{/if}
{#if notice}<p class="notice" role="status">{notice}</p>{/if}

{#if adding}
  <form class="add" onsubmit={add}>
    <label
      >{t('admin.username')}<input
        class="field"
        required
        autocomplete="off"
        bind:value={draft.username}
      /></label
    >
    <label
      >{t('admin.people.displayName')}<input
        class="field"
        autocomplete="off"
        bind:value={draft.displayName}
      /></label
    >
    <label>
      {t('admin.people.firstPassword')}
      <PasswordField
        class="field"
        required
        autocomplete="new-password"
        bind:value={draft.password}
      />
    </label>
    <label>
      {t('admin.people.role')}
      <select class="field" bind:value={draft.role}>
        <option value="user">{t('admin.role.user')}</option>
        <option value="admin">{t('admin.role.admin')}</option>
      </select>
    </label>
    <p class="muted wide">{t('admin.people.addHelp')}</p>
    <div class="wide actions">
      <button class="btn" type="button" onclick={() => (adding = false)}
        >{t('common.cancel')}</button
      >
      <button class="btn primary" type="submit">{t('admin.people.add')}</button>
    </div>
  </form>
{/if}

<div class="split" class:with-detail={open}>
  <div class="list">
    <table>
      <thead>
        <tr>
          <th>{t('admin.username')}</th>
          <th class="hide-narrow">{t('admin.people.groups')}</th>
          <th>{t('admin.people.lastSeen')}</th>
        </tr>
      </thead>
      <tbody>
        {#each people as person (person.id)}
          <tr class:selected={open?.user.id === person.id}>
            <td>
              <button class="link" onclick={() => show(person.id)}>
                <b>{person.username}</b>
                <span class="muted">{person.displayName}</span>
              </button>
              {#if person.role === 'admin'}<span class="badge">{t('admin.role.admin')}</span>{/if}
              {#if person.deleteAt}<span class="badge off">{t('admin.people.leaving')}</span>{/if}
              {#if person.kind === 'e2ee'}<span class="badge">{t('admin.kinds.e2eeShort')}</span
                >{/if}
              {#if person.disabledAt}<span class="badge off">{t('admin.people.off')}</span>{/if}
              {#if person.lockedUntil && person.lockedUntil > Date.now()}<span class="badge off"
                  >{t('admin.people.locked')}</span
                >{/if}
            </td>
            <td class="hide-narrow">{person.groups.join(', ')}</td>
            <td>{relative(person.lastSeenAt)}</td>
          </tr>
        {:else}
          <tr><td colspan="3" class="muted">{t('admin.people.none')}</td></tr>
        {/each}
      </tbody>
    </table>
    {#if total > PAGE}
      <div class="pages">
        <button class="btn" disabled={offset === 0} onclick={() => ((offset -= PAGE), load())}
          >{t('admin.previous')}</button
        >
        <span class="muted"
          >{t('admin.range', { from: offset + 1, to: Math.min(offset + PAGE, total), total })}</span
        >
        <button
          class="btn"
          disabled={offset + PAGE >= total}
          onclick={() => ((offset += PAGE), load())}>{t('admin.next')}</button
        >
      </div>
    {/if}
  </div>

  {#if open}
    {@const user = open.user}
    <section class="detail" aria-label={user.username}>
      <header>
        <h2>{user.displayName}</h2>
        <button class="icon-btn" aria-label={t('common.close')} onclick={() => (open = null)}
          ><Icon name="close" /></button
        >
      </header>
      <dl>
        <dt>{t('admin.username')}</dt>
        <dd>{user.username}</dd>
        <dt>{t('admin.people.created')}</dt>
        <dd>{dateTime(user.createdAt)}</dd>
        <dt>{t('admin.people.groups')}</dt>
        <dd>{user.groups.join(', ') || '–'}</dd>
        <dt>{t('admin.people.email')}</dt>
        <dd>{user.email ?? '–'}</dd>
        {#if user.deleteAt}
          <dt>{t('admin.people.deletion')}</dt>
          <dd>{dateTime(user.deleteAt)}</dd>
        {/if}
        <dt>{t('admin.people.kind')}</dt>
        <dd>{t(`admin.kinds.${user.kind}`)}</dd>
        <dt>{t('admin.people.secondFactor')}</dt>
        <dd>{user.secondFactor ? t('admin.account.on') : t('admin.account.off')}</dd>
      </dl>
      <label class="role">
        {t('admin.people.role')}
        <select
          class="field"
          value={user.role}
          onchange={(e) => patch({ role: e.currentTarget.value }, t('admin.people.saved'))}
        >
          <option value="user">{t('admin.role.user')}</option>
          <option value="admin">{t('admin.role.admin')}</option>
        </select>
      </label>
      <div class="actions">
        <button class="btn" onclick={rename}>{t('admin.people.rename')}</button>
        {#if user.kind === 'standard'}
          <button class="btn" onclick={resetPassword}>{t('admin.people.resetPassword')}</button>
        {/if}
        <button class="btn" onclick={signOutEverywhere} disabled={user.devices === 0}
          >{t('admin.people.signOutAll')}</button
        >
        {#if user.secondFactor}
          <button class="btn" onclick={resetFactors}>{t('admin.people.resetFactors')}</button>
        {/if}
        {#if user.lockedUntil && user.lockedUntil > Date.now()}
          <button class="btn" onclick={() => patch({ unlock: true }, t('admin.people.unlocked'))}
            >{t('admin.people.unlock')}</button
          >
        {/if}
        <button class="btn" class:danger={!user.disabledAt} onclick={toggleOff}>
          {user.disabledAt ? t('admin.people.enable') : t('admin.people.disable')}
        </button>
        <button class="btn danger" onclick={remove} disabled={user.id === auth.session?.user.id}
          >{t('admin.people.delete')}</button
        >
      </div>
      <h3>{t('admin.people.devices', { count: open.deviceList.length })}</h3>
      <ul class="devices">
        {#each open.deviceList as device (device.id)}
          <li>
            <b>{device.name}</b><span class="muted"
              >{device.platform}, {relative(device.lastSeenAt)}</span
            >
          </li>
        {:else}
          <li class="muted">{t('admin.people.noDevices')}</li>
        {/each}
      </ul>
      <h3>{t('admin.policy.title')}</h3>
      <Policy path="/admin/users/{user.id}/policy" level="user" />
    </section>
  {/if}
</div>

<style>
  .bar {
    display: flex;
    gap: var(--space-2);
    margin-bottom: var(--space-3);
  }
  .bar input {
    flex: 1;
    max-width: 360px;
  }
  .add {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
    gap: var(--space-3);
    margin-bottom: var(--space-4);
    padding: var(--space-4);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-popover);
    background: var(--color-surface);
  }
  .add label,
  .role {
    display: grid;
    gap: var(--space-1);
  }
  .wide {
    grid-column: 1 / -1;
    margin: 0;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: var(--space-2);
  }
  .detail .actions {
    justify-content: flex-start;
    margin: var(--space-3) 0;
  }
  .split {
    display: grid;
    gap: var(--space-4);
    align-items: start;
  }
  .split.with-detail {
    grid-template-columns: minmax(0, 1fr) minmax(280px, 380px);
  }
  table {
    width: 100%;
    border-collapse: collapse;
  }
  th {
    text-align: left;
    font-weight: 600;
    color: var(--color-text-muted);
    font-size: 13px;
  }
  th,
  td {
    padding: var(--space-2);
    border-bottom: 1px solid var(--color-border);
    vertical-align: top;
  }
  tr.selected td {
    background: var(--color-selection);
  }
  .link {
    display: inline-grid;
    padding: 0;
    border: 0;
    background: none;
    text-align: left;
  }
  .badge {
    display: inline-block;
    margin: 2px 4px 0 0;
    padding: 0 6px;
    border-radius: 999px;
    font-size: 12px;
    background: var(--color-surface-2);
  }
  .badge.off {
    color: var(--color-danger);
  }
  .pages {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    margin-top: var(--space-3);
  }
  .detail {
    padding: var(--space-4);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-popover);
    background: var(--color-surface);
  }
  .detail header {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }
  h2 {
    margin: 0;
    font-size: 18px;
  }
  h3 {
    margin: var(--space-4) 0 var(--space-2);
    font-size: 14px;
  }
  dl {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: var(--space-1) var(--space-3);
  }
  dt {
    color: var(--color-text-muted);
  }
  dd {
    margin: 0;
  }
  .devices {
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .devices li {
    display: grid;
    padding: var(--space-1) 0;
  }
  .problem {
    color: var(--color-danger);
  }
  .notice {
    color: var(--color-accent-text);
  }
  @media (max-width: 1000px) {
    .split.with-detail {
      grid-template-columns: 1fr;
    }
    .hide-narrow {
      display: none;
    }
  }
</style>
