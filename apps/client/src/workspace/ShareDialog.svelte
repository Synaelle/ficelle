<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Who a folder or note is shared with. The owner adds people by username
     and groups they are in, picks edit or read, removes them, stops
     sharing. Others see who is in it and can leave. For e2ee accounts each
     person shows the fingerprint of their key, to compare with them, and
     people the owner did not choose here (through a group, another device,
     a changed key) wait for the owner to let them in. -->
<script lang="ts">
  import { onMount } from 'svelte'
  import { t } from '../lib/i18n.svelte'
  import Icon from '../lib/icons/Icon.svelte'
  import { modal } from '../lib/modal'
  import { overlay } from '../lib/overlay.svelte'
  import {
    sharing,
    type Group,
    type Member,
    type MemberChoice,
    type ShareTarget,
    type Waiting,
  } from '../lib/sharing.svelte'
  import { isSyncError, sync } from '../lib/sync.svelte'

  let { target }: { target: ShareTarget } = $props()

  const space = $derived('space' in target ? sharing.space(target.space) : undefined)
  const owner = $derived(!space || space.role === 'owner')
  const e2ee = $derived(space ? space.kind === 'e2ee' : sync.status.e2ee)

  let members: Member[] = $state([])
  let waiting: Waiting[] = $state([])
  let groups: Group[] = $state([])
  let username = $state('')
  let busy = $state(false)
  let error: string | null = $state(null)
  let loaded = $state(false)

  onMount(() => void load())

  async function load() {
    try {
      if ('space' in target) members = await sharing.members(target.space)
      else
        members = [
          {
            name: sync.status.username ?? '',
            role: 'owner',
          },
        ]
      if (owner) groups = await sharing.groups()
      if (owner && e2ee && 'space' in target) waiting = await sharing.waiting(target.space)
    } catch (err) {
      show(err)
    }
    loaded = true
  }

  function show(err: unknown) {
    const code = isSyncError(err) ? err.code : 'unknown'
    const known = ['offline', 'notFound', 'signedOut', 'e2eeOnly', 'already']
    error = t(`share.error.${known.includes(code) ? code : 'unknown'}`)
  }

  async function addPerson(event: SubmitEvent) {
    event.preventDefault()
    const name = username.trim()
    if (!name) return
    error = null
    try {
      const person = await sharing.find(name)
      if (
        members.some((m) => m.userId === person.userId) ||
        person.username === sync.status.username
      )
        return show({ code: 'already', message: '' })
      if (e2ee && person.kind !== 'e2ee') return show({ code: 'e2eeOnly', message: '' })
      members = [
        ...members,
        {
          userId: person.userId,
          name: person.username,
          displayName: person.displayName,
          role: 'edit',
          publicKey: person.publicKey,
          fingerprint: person.fingerprint,
        },
      ]
      username = ''
    } catch (err) {
      show(err)
    }
  }

  function addGroup(id: string) {
    const group = groups.find((g) => g.id === id)
    if (!group || members.some((m) => m.groupId === id)) return
    members = [...members, { groupId: group.id, name: group.name, role: 'edit' }]
  }

  const choices = (): MemberChoice[] =>
    members
      .filter((m) => m.role !== 'owner')
      .map((m) => ({
        userId: m.userId,
        groupId: m.groupId,
        role: m.role === 'read' ? 'read' : 'edit',
      }))

  async function allow(who: Waiting) {
    if (!('space' in target)) return
    try {
      await sharing.allow(target.space, who)
      waiting = waiting.filter((w) => w.userId !== who.userId)
    } catch (err) {
      show(err)
    }
  }

  async function save() {
    busy = true
    error = null
    try {
      if ('item' in target) await sharing.create(target.item, choices())
      else await sharing.setMembers(target.space, choices())
      sharing.close()
    } catch (err) {
      show(err)
    }
    busy = false
  }

  async function stop() {
    if (!('space' in target)) return
    const yes = await overlay.ask(t('share.confirmStop', { name: target.name }), {
      confirmLabel: t('share.stop'),
      danger: true,
    })
    if (!yes) return
    busy = true
    try {
      await sharing.stop(target.space)
      sharing.close()
    } catch (err) {
      show(err)
    }
    busy = false
  }

  async function leave() {
    if (!('space' in target)) return
    const yes = await overlay.ask(t('share.confirmLeave', { name: target.name }), {
      confirmLabel: t('share.leave'),
      danger: true,
    })
    if (!yes) return
    busy = true
    try {
      await sharing.leave(target.space)
      sharing.close()
    } catch (err) {
      show(err)
    }
    busy = false
  }
</script>

<div
  class="backdrop"
  role="presentation"
  onclick={(e) => e.target === e.currentTarget && sharing.close()}
>
  <div
    class="dialog"
    role="dialog"
    aria-modal="true"
    aria-label={t('share.title', { name: target.name })}
    use:modal={{ onclose: () => sharing.close() }}
  >
    <header>
      <h2>{t('share.title', { name: target.name })}</h2>
      <button class="icon-btn" aria-label={t('common.close')} onclick={() => sharing.close()}
        ><Icon name="close" /></button
      >
    </header>

    {#if space && !owner}
      <p class="muted">
        {space.role === 'read'
          ? t('share.sharedRead', { owner: space.ownerName })
          : t('share.sharedEdit', { owner: space.ownerName })}
      </p>
    {/if}
    {#if e2ee}
      <p class="muted small">{t('share.e2eeNote')}</p>
    {/if}

    <ul class="members" aria-busy={!loaded}>
      {#each members as member, i (member.userId ?? member.groupId ?? `owner${i}`)}
        <li>
          <Icon name={member.groupId ? 'groups' : 'user'} size={16} />
          <div class="who">
            <span class="name"
              >{member.displayName && member.displayName !== member.name
                ? `${member.displayName} (${member.name})`
                : member.name}</span
            >
            {#if member.fingerprint}
              <span class="fingerprint" title={t('share.fingerprint')}>{member.fingerprint}</span>
            {:else if e2ee && member.userId && member.role !== 'owner'}
              <span class="muted small">{t('share.noKeyYet')}</span>
            {/if}
          </div>
          {#if member.role === 'owner'}
            <span class="muted small">{t('share.role.owner')}</span>
          {:else if owner}
            <select
              class="field role"
              aria-label={t('share.roleOf', { name: member.name })}
              bind:value={member.role}
            >
              <option value="edit">{t('share.role.edit')}</option>
              <option value="read">{t('share.role.read')}</option>
            </select>
            <button
              class="icon-btn"
              aria-label={t('share.remove', { name: member.name })}
              onclick={() => (members = members.filter((_, j) => j !== i))}
              ><Icon name="close" size={16} /></button
            >
          {:else}
            <span class="muted small">{t(`share.role.${member.role}`)}</span>
          {/if}
        </li>
      {/each}
    </ul>

    {#if waiting.length}
      <section class="waiting" aria-label={t('share.waitingTitle')}>
        <h3>{t('share.waitingTitle')}</h3>
        <p class="muted small">{t('share.waitingHelp')}</p>
        <ul class="members">
          {#each waiting as who (who.userId)}
            <li>
              <Icon name="user" size={16} />
              <div class="who">
                <span class="name">{who.name || who.userId}</span>
                {#if who.fingerprint}
                  <span class="fingerprint" title={t('share.fingerprint')}>{who.fingerprint}</span>
                {/if}
                {#if who.changed}<span class="warning small">{t('share.keyChanged')}</span>{/if}
              </div>
              <button class="btn" disabled={busy} onclick={() => void allow(who)}
                >{t('share.allow')}</button
              >
            </li>
          {/each}
        </ul>
      </section>
    {/if}

    {#if owner}
      <form class="add" onsubmit={addPerson}>
        <input
          class="field"
          placeholder={t('share.username')}
          aria-label={t('share.username')}
          autocomplete="off"
          autocapitalize="none"
          data-autofocus
          bind:value={username}
        />
        <button class="btn" type="submit" disabled={!username.trim()}>{t('share.add')}</button>
      </form>
      {#if groups.length}
        <select
          class="field"
          aria-label={t('share.addGroup')}
          value=""
          onchange={(e) => {
            addGroup(e.currentTarget.value)
            e.currentTarget.value = ''
          }}
        >
          <option value="" disabled>{t('share.addGroup')}</option>
          {#each groups as group (group.id)}
            <option value={group.id}>{group.name}</option>
          {/each}
        </select>
      {/if}
    {/if}

    {#if error}<p class="error" role="alert">{error}</p>{/if}

    <div class="actions">
      {#if space && owner}
        <button class="btn danger" disabled={busy} onclick={stop}>{t('share.stop')}</button>
      {:else if space}
        <button class="btn danger" disabled={busy} onclick={leave}>{t('share.leave')}</button>
      {/if}
      <span class="grow"></span>
      {#if owner}
        <button class="btn" onclick={() => sharing.close()}>{t('common.cancel')}</button>
        <button
          class="btn primary"
          disabled={busy || !loaded || (!space && members.length < 2)}
          onclick={save}>{space ? t('share.save') : t('share.share')}</button
        >
      {:else}
        <button class="btn" onclick={() => sharing.close()}>{t('common.close')}</button>
      {/if}
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 50;
    display: grid;
    place-items: center;
    padding: var(--space-4);
    background: rgb(0 0 0 / 0.5);
  }
  .dialog {
    width: min(480px, 100%);
    max-height: calc(100vh - 2 * var(--space-4));
    overflow-y: auto;
    display: grid;
    gap: var(--space-3);
    padding: var(--space-5);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-window);
  }
  header {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  h2 {
    flex: 1;
    margin: 0;
    font-size: 16px;
    overflow-wrap: anywhere;
  }
  p {
    margin: 0;
  }
  .small {
    font-size: 12px;
  }
  .members {
    margin: 0;
    padding: 0;
    list-style: none;
    display: grid;
    gap: var(--space-1);
  }
  .members li {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-height: 40px;
  }
  .who {
    flex: 1;
    min-width: 0;
    display: grid;
  }
  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .waiting {
    display: grid;
    gap: var(--space-2);
  }
  .waiting h3 {
    margin: 0;
    font-size: 14px;
  }
  .warning {
    color: var(--color-warning);
  }
  .fingerprint {
    font-family: var(--font-mono, monospace);
    font-size: 12px;
    color: var(--color-text-muted);
  }
  .role {
    width: auto;
  }
  .add {
    display: flex;
    gap: var(--space-2);
  }
  .add .field {
    flex: 1;
    min-width: 0;
  }
  .error {
    color: var(--color-danger);
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }
  .grow {
    flex: 1;
  }
</style>
