<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- The admin panel: a section list beside the open section, a bar across
     the top on narrow screens. The section is kept in the address, a reload
     stays where it was. -->
<script lang="ts">
  import { brand } from '@app/brand'
  import { t } from '../lib/i18n.svelte'
  import Icon from '../lib/icons/Icon.svelte'
  import type { IconName } from '../lib/icons/sets'
  import { auth, signOut, type ServerInfo } from './api.svelte'
  import Account from './Account.svelte'
  import AuditLog from './AuditLog.svelte'
  import Groups from './Groups.svelte'
  import Invites from './Invites.svelte'
  import Overview from './Overview.svelte'
  import People from './People.svelte'
  import Storage from './Storage.svelte'
  import Extensions from './Extensions.svelte'
  import Backups from './Backups.svelte'
  import Feedback from './Feedback.svelte'
  import { api } from './api.svelte'

  let { info, admin }: { info: ServerInfo; admin: boolean } = $props()

  type Section =
    | 'overview'
    | 'people'
    | 'invites'
    | 'groups'
    | 'storage'
    | 'backups'
    | 'extensions'
    | 'feedback'
    | 'log'
    | 'account'
  const SECTIONS: [Section, IconName][] = [
    ['overview', 'dashboard'],
    ['people', 'user'],
    ['invites', 'invite'],
    ['groups', 'groups'],
    ['storage', 'database'],
    ['backups', 'backup'],
    ['extensions', 'palette'],
    ['feedback', 'feedback'],
    ['log', 'log'],
    ['account', 'shield'],
  ]
  /** What a server for its admin alone shows: nothing about other people. */
  const PERSONAL: Section[] = ['overview', 'storage', 'backups', 'log', 'account']
  /** Members the admins named read the feedback inbox too. */
  let reader = $state(false)
  /** The server is for its admin alone, from the overview. */
  let personal = $state(false)
  const shown = $derived(
    admin
      ? personal
        ? SECTIONS.filter(([s]) => PERSONAL.includes(s))
        : SECTIONS
      : SECTIONS.filter(([s]) => s === 'account' || (reader && s === 'feedback')),
  )
  const fromHash = (): Section =>
    shown.find(([s]) => `#${s}` === location.hash)?.[0] ?? shown[0]?.[0] ?? 'account'
  let section: Section = $state(fromHash())

  // svelte-ignore state_referenced_locally
  if (admin) {
    void api<{ personal: boolean }>('GET', '/admin/overview')
      .then((overview) => {
        personal = overview.personal
        section = fromHash()
      })
      .catch(() => {})
  }
  // svelte-ignore state_referenced_locally
  if (!admin) {
    void api<{ inbox: boolean }>('GET', '/admin/feedback/access')
      .then((access) => {
        reader = access.inbox
        section = fromHash()
      })
      .catch(() => {})
  }

  function go(next: Section) {
    section = next
    history.replaceState(null, '', `#${next}`)
  }
</script>

<svelte:window onhashchange={() => (section = fromHash())} />

<div class="panel">
  <header>
    <b
      >{admin
        ? t('admin.title', { app: brand.displayName })
        : t('admin.memberTitle', { app: brand.displayName })}</b
    >
    <span class="muted version">{info.version}</span>
    <span class="who muted">{auth.session?.user.username}</span>
    <button class="btn" onclick={signOut}
      ><Icon name="signOut" size={16} />{t('admin.signOut')}</button
    >
  </header>
  <nav aria-label={t('admin.sections')}>
    {#each shown as [s, icon] (s)}
      <button aria-current={section === s ? 'page' : undefined} onclick={() => go(s)}>
        <Icon name={icon} size={16} />{t(`admin.${s}.title`)}
      </button>
    {/each}
  </nav>
  <main>
    {#if auth.session?.user.factorRequired}
      <p class="notice" role="status">{t('admin.factorRequired')}</p>
    {/if}
    <h1>{t(`admin.${section}.title`)}</h1>
    {#if section === 'overview'}
      <Overview onpersonal={(on) => (personal = on)} />
    {:else if section === 'people'}
      <People />
    {:else if section === 'invites'}
      <Invites />
    {:else if section === 'groups'}
      <Groups />
    {:else if section === 'storage'}
      <Storage />
    {:else if section === 'backups'}
      <Backups />
    {:else if section === 'extensions'}
      <Extensions />
    {:else if section === 'feedback'}
      <Feedback {admin} />
    {:else if section === 'log'}
      <AuditLog />
    {:else}
      <Account />
    {/if}
  </main>
</div>

<style>
  .notice {
    margin: 0 0 var(--space-4);
    padding: var(--space-3);
    border-left: 3px solid var(--color-accent);
    background: var(--color-surface);
  }
  .panel {
    height: 100%;
    display: grid;
    grid-template: 'header header' auto 'nav main' 1fr / 210px 1fr;
  }
  header {
    grid-area: header;
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-2) var(--space-4);
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface);
  }
  .version {
    font-family: var(--font-mono);
    font-size: 12px;
  }
  .who {
    margin-left: auto;
  }
  nav {
    grid-area: nav;
    display: grid;
    align-content: start;
    gap: 2px;
    padding: var(--space-3);
    border-right: 1px solid var(--color-border);
  }
  nav button {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    height: 36px;
    padding: 0 var(--space-3);
    border: 0;
    border-radius: var(--radius-control);
    background: none;
    text-align: left;
  }
  nav button[aria-current] {
    background: var(--color-selection);
  }
  main {
    grid-area: main;
    overflow: auto;
    padding: var(--space-4) var(--space-5) var(--space-6);
  }
  h1 {
    margin: 0 0 var(--space-4);
    font-size: 22px;
  }
  @media (max-width: 760px) {
    .panel {
      grid-template: 'header' auto 'nav' auto 'main' 1fr / 1fr;
    }
    nav {
      display: flex;
      overflow-x: auto;
      border-right: 0;
      border-bottom: 1px solid var(--color-border);
    }
    nav button {
      flex: none;
    }
    .version {
      display: none;
    }
    main {
      padding: var(--space-4);
    }
  }
</style>
