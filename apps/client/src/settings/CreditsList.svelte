<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- What Ficelle builds on (lib/credits.ts, the same as CREDITS.md): plain
     groups on the desktop, the phone's rounded groups with `phone`. -->
<script lang="ts">
  // names, not links: the app has no way yet to open a page in the browser,
  // a link would load it in the app's own window. CREDITS.md links them.
  import { t } from '../lib/i18n.svelte'
  import { CREDIT_GROUPS, MAKER } from '../lib/credits'

  let { phone = false }: { phone?: boolean } = $props()
</script>

<div class="credits" class:phone>
  <p class="intro">
    {t('settings.creditsMadeBy')}
    <strong>{MAKER.name}</strong>.
    {t('settings.creditsThanks')}
  </p>
  {#each CREDIT_GROUPS as group (group.title)}
    <h4>{group.title}</h4>
    <ul>
      {#each group.credits as c (c.name)}
        <li>
          <strong title={c.url}>{c.name}</strong>
          <span class="use">{c.use}</span>
          <span class="license">{c.license}</span>
        </li>
      {/each}
    </ul>
  {/each}
</div>

<style>
  .credits {
    display: grid;
    gap: var(--space-2);
    width: 100%;
    min-width: 0;
    font-size: 13px;
  }
  .intro {
    margin: 0;
    color: var(--color-text-muted);
    line-height: 1.45;
  }
  h4 {
    margin: var(--space-3) 0 0;
    font-size: 13px;
    font-weight: 600;
  }
  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }
  li {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 0 var(--space-3);
    padding: var(--space-1) 0;
    line-height: 1.4;
  }
  strong {
    font-weight: 500;
    overflow-wrap: anywhere;
  }
  .use {
    grid-column: 1;
    grid-row: 2;
    color: var(--color-text-muted);
  }
  .license {
    grid-column: 2;
    grid-row: 1 / span 2;
    align-self: center;
    color: var(--color-text-muted);
    text-align: right;
    white-space: nowrap;
  }

  /* the phone's lists: a rounded surface per group, hairlines between rows */
  .phone {
    gap: 0;
    font-size: 15px;
  }
  .phone .intro {
    margin: var(--space-2) var(--space-4) 0;
    font-size: 13px;
  }
  .phone h4 {
    margin: var(--space-5) var(--space-4) var(--space-2);
    color: var(--color-text-muted);
    font-size: 13px;
    font-weight: 400;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .phone ul {
    overflow: hidden;
    border-radius: var(--radius-popover);
    background: var(--color-surface);
  }
  .phone li {
    padding: var(--space-2) var(--space-4);
  }
  .phone li + li {
    border-top: 1px solid var(--color-border);
  }
  .phone .use {
    font-size: 13px;
  }
  .phone .license {
    font-size: 13px;
  }
</style>
