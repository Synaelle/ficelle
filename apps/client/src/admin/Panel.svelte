<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- The admin panel: a section list beside the open section, a bar across
     the top on narrow screens. The section is kept in the address, a reload
     stays where it was. -->
<script lang="ts">
  import { brand } from '@app/brand'
  import logo from '../../../../packages/ui/brand/app-icon.svg?url'
  import { t } from '../lib/i18n.svelte'
  import Icon from '../lib/icons/Icon.svelte'
  import type { IconName } from '../lib/icons/sets'
  import { auth, signOut, type ServerInfo } from './api.svelte'
  import Account from './Account.svelte'
  import AuditLog from './AuditLog.svelte'
  import Overview from './Overview.svelte'
  import OnePerson from './OnePerson.svelte'
  import Storage from './Storage.svelte'
  import Backups from './Backups.svelte'
  import Feedback from './Feedback.svelte'
  import WebVersion from './WebVersion.svelte'
  import { api, sectionHref, type WebView } from './api.svelte'
  import { errorText } from './format'

  let { info, admin }: { info: ServerInfo; admin: boolean } = $props()

  type Section = 'overview' | 'onePerson' | 'web' | 'storage' | 'backups' | 'feedback' | 'security'
  const SERVER: [Section, IconName][] = [
    ['overview', 'dashboard'],
    ['onePerson', 'user'],
    ['web', 'globe'],
    ['storage', 'database'],
    ['backups', 'backup'],
  ]
  const PEOPLE: [Section, IconName][] = [
    ['feedback', 'feedback'],
    ['security', 'shield'],
  ]
  /** Members the admins named read the feedback inbox too. */
  let reader = $state(false)
  /** More than one account here: going to one has its own section. */
  let several = $state(false)
  /** Why the sections that depend on the server could not be shown. */
  let problem = $state('')
  /** The web version's state and the new reports, beside their sections. */
  let web: WebView | null = $state(null)
  let newReports = $state(0)
  const server = $derived(admin ? SERVER.filter(([s]) => s !== 'onePerson' || several) : [])
  const people = $derived(
    admin ? PEOPLE : PEOPLE.filter(([s]) => s === 'security' || (reader && s === 'feedback')),
  )
  const shown = $derived([...server, ...people])
  // the log and the account had sections of their own before
  const OLD: Record<string, Section> = { '#log': 'security', '#account': 'security' }
  const fromHash = (): Section =>
    OLD[location.hash] ??
    shown.find(([s]) => `#${s}` === location.hash)?.[0] ??
    shown[0]?.[0] ??
    'security'
  let section: Section = $state(fromHash())

  function countAccounts() {
    void api<{ users: number }>('GET', '/admin/overview')
      .then((overview) => {
        several = overview.users > 1
        section = fromHash()
      })
      .catch((err: unknown) => (problem = errorText(err)))
  }

  function loadTags() {
    void api<WebView>('GET', '/admin/web')
      .then((view) => (web = view))
      .catch((err: unknown) => (problem = errorText(err)))
    void api<{ counts: { new: number } }>('GET', '/admin/feedback/reports?status=new&limit=1')
      .then((page) => (newReports = page.counts.new))
      .catch((err: unknown) => (problem = errorText(err)))
  }
  // svelte-ignore state_referenced_locally
  if (admin) {
    countAccounts()
    loadTags()
  }
  // svelte-ignore state_referenced_locally
  if (!admin) {
    void api<{ inbox: boolean }>('GET', '/admin/feedback/access')
      .then((access) => {
        reader = access.inbox
        section = fromHash()
      })
      .catch((err: unknown) => (problem = errorText(err)))
  }

  function go(next: Section) {
    section = next
    history.replaceState(null, '', sectionHref(next))
  }
</script>

<svelte:window onhashchange={() => (section = fromHash())} />

{#snippet entry([s, icon]: [Section, IconName])}
  <button aria-current={section === s ? 'page' : undefined} onclick={() => go(s)}>
    <Icon name={icon} size={16} /><span class="name">{t(`admin.${s}.title`)}</span>
    {#if s === 'web' && web}
      <span class={['tag', web.access !== 'off' && 'on']}
        >{web.access === 'off' ? t('admin.account.off') : t('admin.account.on')}</span
      >
    {:else if s === 'feedback' && newReports > 0}
      <span class="tag count">{newReports}</span>
    {/if}
  </button>
{/snippet}

<div class="panel">
  <header>
    <img class="logo" src={logo} alt="" />
    <b
      >{admin
        ? t('admin.title', { app: brand.displayName })
        : t('admin.memberTitle', { app: brand.displayName })}</b
    >
    <span class="muted version">{info.version}</span>
    <span class="who muted">{auth.session?.user.username}</span>
    <button class="btn quiet" onclick={signOut}
      ><Icon name="signOut" size={16} />{t('admin.signOut')}</button
    >
  </header>
  <nav aria-label={t('admin.sections')}>
    {#if server.length}
      <span class="group">{t('admin.groupServer')}</span>
      {#each server as item (item[0])}{@render entry(item)}{/each}
      <span class="group">{t('admin.groupPeople')}</span>
    {/if}
    {#each people as item (item[0])}{@render entry(item)}{/each}
  </nav>
  <main>
    {#if auth.session?.user.factorRequired}
      <p class="notice" role="status">{t('admin.factorRequired')}</p>
    {/if}
    {#if problem}<p class="problem" role="alert">{problem}</p>{/if}
    <div class="dash-page">
      <div class="dash-head"><h1>{t(`admin.${section}.title`)}</h1></div>
      {#if section === 'overview'}
        <Overview {several} ongo={() => go('onePerson')} />
      {:else if section === 'onePerson'}
        <OnePerson
          ondone={() => {
            several = false
            go('overview')
          }}
        />
      {:else if section === 'web'}
        <WebVersion onchange={(view) => (web = view)} />
      {:else if section === 'storage'}
        <Storage />
      {:else if section === 'backups'}
        <Backups />
      {:else if section === 'feedback'}
        <Feedback {admin} />
      {:else}
        <Account />
        {#if admin}<AuditLog />{/if}
      {/if}
    </div>
  </main>
</div>

<style>
  .problem {
    color: var(--color-danger);
  }
  .notice {
    margin: 0 0 var(--space-4);
    padding: var(--space-3);
    border-left: 3px solid var(--color-accent);
    background: var(--color-surface);
  }
  .panel {
    height: 100%;
    display: grid;
    grid-template: 'header header' auto 'nav main' 1fr / 200px 1fr;
  }
  header {
    grid-area: header;
    display: flex;
    align-items: center;
    gap: var(--space-3);
    height: 44px;
    padding: 0 var(--space-4);
    border-bottom: 1px solid var(--color-border);
  }
  .logo {
    width: 18px;
    height: 18px;
  }
  .version {
    font-family: var(--font-mono);
    font-size: 12px;
  }
  .who {
    margin-left: auto;
    font-size: 13px;
  }
  nav {
    grid-area: nav;
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: var(--space-3) var(--space-2);
    border-right: 1px solid var(--color-border);
  }
  .group {
    padding: var(--space-3) 10px var(--space-1);
    color: var(--color-text-muted);
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }
  nav button {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 30px;
    padding: 0 10px;
    border: 0;
    border-radius: var(--radius-control);
    background: none;
    text-align: left;
  }
  nav button :global(svg) {
    flex: none;
    color: var(--color-text-muted);
  }
  nav button[aria-current] {
    background: var(--color-selection);
  }
  nav button[aria-current] :global(svg) {
    color: var(--color-accent-text);
  }
  .name {
    flex: 1;
  }
  .tag {
    color: var(--color-text-muted);
    font-size: 12px;
  }
  .tag.on {
    color: var(--color-success);
  }
  .tag.count {
    color: var(--color-accent-text);
    font-weight: 600;
  }
  main {
    grid-area: main;
    overflow: auto;
    padding: 20px 28px 32px;
  }
  @media (max-width: 760px) {
    .panel {
      grid-template: 'header' auto 'nav' auto 'main' 1fr / 1fr;
    }
    nav {
      flex-direction: row;
      overflow-x: auto;
      padding: 6px var(--space-2);
      border-right: 0;
      border-bottom: 1px solid var(--color-border);
    }
    .group {
      display: none;
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
