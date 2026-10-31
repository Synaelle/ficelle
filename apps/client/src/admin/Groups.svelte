<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Groups and their members. -->
<script lang="ts">
  import { t } from '../lib/i18n.svelte'
  import Icon from '../lib/icons/Icon.svelte'
  import { overlay } from '../lib/overlay.svelte'
  import { api, type AdminUser, type Group, type GroupMember } from './api.svelte'
  import { date, errorText } from './format'
  import Policy from './Policy.svelte'

  let groups: Group[] = $state([])
  let open: { group: Group; memberList: GroupMember[] } | null = $state(null)
  let people: AdminUser[] = $state([])
  let adding = $state('')
  let problem = $state('')

  async function run(action: () => Promise<unknown>) {
    problem = ''
    try {
      await action()
    } catch (err) {
      problem = errorText(err)
    }
  }

  const load = () =>
    run(async () => {
      groups = await api<Group[]>('GET', '/admin/groups')
    })

  const show = (id: string) =>
    run(async () => {
      open = await api('GET', `/admin/groups/${id}`)
      if (people.length === 0) {
        people = (await api<{ items: AdminUser[] }>('GET', '/admin/users?limit=200')).items
      }
    })

  async function create() {
    const name = await overlay.askText(t('admin.groups.namePrompt'))
    if (!name?.trim()) return
    await run(async () => {
      const group = await api<Group>('POST', '/admin/groups', { name })
      await load()
      await show(group.id)
    })
  }

  async function rename() {
    if (!open) return
    const id = open.group.id
    const name = await overlay.askText(t('admin.groups.namePrompt'), open.group.name)
    if (!name?.trim()) return
    await run(async () => {
      await api('PATCH', `/admin/groups/${id}`, { name })
      await Promise.all([load(), show(id)])
    })
  }

  async function remove() {
    if (!open) return
    const { id, name } = open.group
    const yes = await overlay.ask(t('admin.groups.deleteConfirm', { name }), {
      confirmLabel: t('admin.groups.delete'),
      danger: true,
    })
    if (!yes) return
    await run(async () => {
      await api('DELETE', `/admin/groups/${id}`)
      open = null
      await load()
    })
  }

  async function addMember(event: SubmitEvent) {
    event.preventDefault()
    if (!open) return
    const id = open.group.id
    const person = people.find((p) => p.username === adding.trim().toLowerCase())
    if (!person) {
      problem = t('admin.groups.unknownPerson', { name: adding })
      return
    }
    await run(async () => {
      await api('PUT', `/admin/groups/${id}/members/${person.id}`)
      adding = ''
      await Promise.all([load(), show(id)])
    })
  }

  async function removeMember(userId: string) {
    if (!open) return
    const id = open.group.id
    await run(async () => {
      await api('DELETE', `/admin/groups/${id}/members/${userId}`)
      await Promise.all([load(), show(id)])
    })
  }

  void load()
</script>

<p class="muted">{t('admin.groups.help')}</p>
<div class="bar">
  <button class="btn primary" onclick={create}
    ><Icon name="plus" size={16} />{t('admin.groups.create')}</button
  >
</div>
{#if problem}<p class="problem" role="alert">{problem}</p>{/if}

<div class="split">
  <ul class="groups">
    {#each groups as group (group.id)}
      <li>
        <button
          aria-current={open?.group.id === group.id ? 'true' : undefined}
          onclick={() => show(group.id)}
        >
          <b>{group.name}</b>
          <span class="muted">{t('admin.groups.memberCount', { count: group.members })}</span>
        </button>
      </li>
    {:else}
      <li class="muted">{t('admin.groups.none')}</li>
    {/each}
  </ul>

  {#if open}
    <section class="detail" aria-label={open.group.name}>
      <header>
        <h2>{open.group.name}</h2>
        <span class="muted">{t('admin.groups.since', { date: date(open.group.createdAt) })}</span>
      </header>
      <div class="actions">
        <button class="btn" onclick={rename}>{t('admin.groups.rename')}</button>
        <button class="btn danger" onclick={remove}>{t('admin.groups.delete')}</button>
      </div>
      <form class="add" onsubmit={addMember}>
        <input
          class="field"
          list="people"
          placeholder={t('admin.groups.addPlaceholder')}
          aria-label={t('admin.groups.add')}
          bind:value={adding}
        />
        <datalist id="people">
          {#each people.filter((p) => !open?.memberList.some((m) => m.userId === p.id)) as p (p.id)}
            <option value={p.username}>{p.displayName}</option>
          {/each}
        </datalist>
        <button class="btn" type="submit" disabled={!adding.trim()}>{t('admin.groups.add')}</button>
      </form>
      <ul class="members">
        {#each open.memberList as member (member.userId)}
          <li>
            <span><b>{member.username}</b> <span class="muted">{member.displayName}</span></span>
            <button class="btn" onclick={() => removeMember(member.userId)}
              >{t('admin.groups.removeMember')}</button
            >
          </li>
        {:else}
          <li class="muted">{t('admin.groups.noMembers')}</li>
        {/each}
      </ul>
      <h3>{t('admin.policy.title')}</h3>
      <Policy path="/admin/groups/{open.group.id}/policy" level="group" />
    </section>
  {/if}
</div>

<style>
  .bar {
    margin-bottom: var(--space-3);
  }
  .split {
    display: grid;
    grid-template-columns: minmax(200px, 280px) minmax(0, 1fr);
    gap: var(--space-4);
    align-items: start;
  }
  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .groups button {
    width: 100%;
    display: grid;
    padding: var(--space-2) var(--space-3);
    border: 0;
    border-radius: var(--radius-control);
    background: none;
    text-align: left;
  }
  .groups button[aria-current] {
    background: var(--color-selection);
  }
  .detail {
    padding: var(--space-4);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-popover);
    background: var(--color-surface);
  }
  header {
    display: flex;
    align-items: baseline;
    gap: var(--space-3);
  }
  h2 {
    margin: 0;
    font-size: 18px;
  }
  h3 {
    margin: var(--space-4) 0 var(--space-2);
    font-size: 14px;
  }
  .actions,
  .add {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    margin: var(--space-3) 0;
  }
  .add input {
    flex: 1;
    min-width: 160px;
  }
  .members li {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-1) 0;
    border-bottom: 1px solid var(--color-border);
  }
  .problem {
    color: var(--color-danger);
  }
  @media (max-width: 760px) {
    .split {
      grid-template-columns: 1fr;
    }
  }
</style>
