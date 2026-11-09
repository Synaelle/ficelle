<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- The reports sent from here, folded: what each was about, when, how it stands
     and the one answer it got. Each instance is asked with the secrets of
     its own reports only, and nothing else of this app goes with them. A
     report the instance no longer has reads as no longer kept. Settings
     groups, see app.css. -->
<script lang="ts">
  import { backend, type FeedbackAnswer } from '../lib/backend'
  import { forgetSent, loadSent, type SentReport } from '../lib/feedback'
  import { t } from '../lib/i18n.svelte'
  import Icon from '../lib/icons/Icon.svelte'

  let list: SentReport[] = $state(loadSent())
  /** By secret; a secret missing from a read instance is no longer kept. */
  let answers: Record<string, FeedbackAnswer> = $state({})
  /** The instances that answered, and those that could not be reached. */
  let read: string[] = $state([])
  let unreached: string[] = $state([])
  let busy = $state(false)
  let open = $state(false)
  /** Asked once it is opened: a closed list sends nothing. */
  let asked = false

  async function refresh() {
    busy = true
    list = loadSent()
    const byUrl: Record<string, string[]> = {}
    for (const r of list) (byUrl[r.url] ??= []).push(r.secret)
    const found: Record<string, FeedbackAnswer> = {}
    const ok: string[] = []
    const failed: string[] = []
    await Promise.all(
      Object.entries(byUrl).map(async ([url, secrets]) => {
        try {
          const { reports } = await backend.feedbackAnswers(url, secrets)
          for (const r of reports) found[r.secret] = r
          ok.push(url)
        } catch {
          failed.push(url)
        }
      }),
    )
    answers = found
    read = ok
    unreached = failed
    busy = false
  }

  function fold() {
    open = !open
    if (open && !asked) {
      asked = true
      void refresh()
    }
  }

  function forget(secret: string) {
    forgetSent(secret)
    list = loadSent()
  }

  function standing(r: SentReport): string {
    const answer = answers[r.secret]
    if (answer) return t(`feedback.state_${answer.state}`)
    if (unreached.includes(r.url)) return t('feedback.state_unknown')
    if (read.includes(r.url)) return t('feedback.state_gone')
    return ''
  }

  const when = (ms: number) => new Date(ms).toLocaleDateString(undefined, { dateStyle: 'medium' })
</script>

{#if list.length}
  <div class="settings-group wide settings-panel" data-feedback-list aria-busy={busy}>
    <div class="line head">
      <h3>
        <button class="fold" aria-expanded={open} onclick={fold}>
          {t('feedback.sentTitle')} ({list.length})
          <span class="chev" class:open><Icon name="down" size={14} /></span>
        </button>
      </h3>
      {#if open}
        <button class="btn small" disabled={busy} onclick={() => void refresh()}
          >{t('feedback.refresh')}</button
        >
      {/if}
    </div>
    {#if open}
      <p class="settings-help">{t('feedback.sentHelp')}</p>
      <ul>
        {#each list as r (r.secret)}
          {@const answer = answers[r.secret]}
          <li data-feedback-sent-report>
            <div class="line">
              <span class="about">{r.about}</span>
              <span class="state" data-state={answer?.state ?? ''}>{standing(r)}</span>
            </div>
            <div class="line">
              <span class="settings-help">{when(r.sentAt)} · {r.host}</span>
              <button class="btn small" onclick={() => forget(r.secret)}
                >{t('feedback.forget')}</button
              >
            </div>
            {#if answer?.answer}
              <blockquote data-feedback-answer>
                <span class="settings-help">{t('feedback.answer')}</span>
                {answer.answer}
              </blockquote>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
  </div>
{/if}

<style>
  [data-feedback-list] {
    padding: var(--space-1) var(--space-3);
  }
  h3 {
    margin: 0;
    font-size: 14px;
  }
  .fold {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    min-height: 32px;
    padding: 0;
    border: 0;
    background: none;
    color: inherit;
    cursor: pointer;
    font: inherit;
  }
  .chev {
    display: inline-flex;
    transition: transform var(--dur-fast) var(--ease-out);
  }
  .chev.open {
    transform: rotate(180deg);
  }
  @media (prefers-reduced-motion: reduce) {
    .chev {
      transition: none;
    }
  }
  ul {
    display: grid;
    gap: var(--space-3);
    width: min(560px, 100%);
    margin: 0;
    padding: 0;
    list-style: none;
  }
  li {
    display: grid;
    gap: var(--space-1);
    justify-items: start;
    padding-bottom: var(--space-3);
    border-bottom: 1px solid var(--color-border);
  }
  li:last-child {
    padding-bottom: 0;
    border-bottom: 0;
  }
  .head {
    align-items: center;
    width: min(560px, 100%);
  }
  .line {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    align-items: baseline;
    justify-content: space-between;
    width: 100%;
  }
  .about {
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .state {
    color: var(--color-text-muted);
    font-size: 12px;
  }
  .state[data-state='open'],
  .state[data-state='done'] {
    color: var(--color-accent);
  }
  blockquote {
    display: grid;
    gap: var(--space-1);
    margin: 0;
    padding: var(--space-2) var(--space-3);
    border-left: 3px solid var(--color-accent);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .small {
    font-size: 12px;
  }
</style>
