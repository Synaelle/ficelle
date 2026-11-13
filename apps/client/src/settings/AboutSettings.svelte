<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- About, the same on desktop and phone: which app and version, updates, how
     to reach us. Build details and the credits stay folded. -->
<script lang="ts">
  import { brand } from '@app/brand'
  import { updatesHere } from '../lib/appUpdate.svelte'
  import { t } from '../lib/i18n.svelte'
  import Icon from '../lib/icons/Icon.svelte'
  import Knot from '@app/ui/Knot.svelte'
  import { serverProtocol } from '../lib/sync.svelte'
  import ContactLine from './ContactLine.svelte'
  import CreditsList from './CreditsList.svelte'
  import Group from './ui/Group.svelte'
  import Row from './ui/Row.svelte'
  import UpdateCheck from './UpdateCheck.svelte'

  let { phone = false }: { phone?: boolean } = $props()

  let details = $state(false)
  let credits = $state(false)
  /** The linked server's protocol versions, asked when the details open. */
  let serverProtocols: string | null = $state(null)
  $effect(() => {
    if (details) void serverProtocol().then((v) => (serverProtocols = v))
  })
</script>

{#snippet chevron(open: boolean)}
  <span class="chevron" class:open><Icon name="down" size={16} /></span>
{/snippet}

<Group>
  <div class="identity">
    <span class="mark"><Knot size={32} /></span>
    <span class="words">
      <b>{brand.displayName}</b>
      <span class="version">{t('settings.version')} {__APP_VERSION__}</span>
    </span>
  </div>
</Group>

{#if updatesHere}<UpdateCheck />{/if}

<Group>
  <ContactLine />
  <Row label={t('settings.credits')} onclick={() => (credits = !credits)}>
    {@render chevron(credits)}
  </Row>
  {#if credits}<div class="inside fold-rows"><CreditsList {phone} /></div>{/if}
  <Row label={t('settings.details')} onclick={() => (details = !details)}>
    {@render chevron(details)}
  </Row>
  {#if details}
    <div class="fold-rows">
      <Row label={t('settings.buildDate')}><span class="mono">{__APP_BUILD_DATE__}</span></Row>
      <Row label={t('settings.protocol')}><span class="mono">{__APP_PROTOCOL__}</span></Row>
      {#if serverProtocols}
        <Row label={t('settings.serverProtocol')}><span class="mono">{serverProtocols}</span></Row>
      {/if}
    </div>
  {/if}
</Group>

<style>
  .identity {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-3);
  }
  .mark {
    --knot-behind: var(--color-surface-2);
    display: grid;
    flex: none;
    place-items: center;
    width: 48px;
    height: 48px;
    border-radius: var(--radius-control);
    background: var(--knot-behind);
  }
  :global(:root[data-black='true']) .mark {
    --knot-behind: transparent;
  }
  .words {
    display: grid;
    min-width: 0;
  }
  b {
    font-size: 16px;
  }
  .version {
    color: var(--color-text-muted);
    font-size: 13px;
    overflow-wrap: anywhere;
  }
  .chevron {
    display: grid;
    color: var(--color-text-faint);
    transition: transform var(--dur-fast) var(--ease-out);
  }
  .chevron.open {
    transform: rotate(180deg);
  }
  .inside {
    padding: var(--space-2) var(--space-3) var(--space-3);
  }
  .mono {
    color: var(--color-text-muted);
    font-family: var(--font-mono);
    font-size: 13px;
  }
</style>
