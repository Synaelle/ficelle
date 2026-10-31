<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- The reports sent from here: what each was about, when, how it stands
     and the one answer it got. Each instance is asked with the secrets of
     its own reports only, and nothing else of this app goes with them. A
     report the instance no longer has reads as no longer kept. Settings
     groups, see app.css. -->
<script lang="ts">
  import { onMount } from 'svelte'
  import { backend, type FeedbackAnswer } from '../lib/backend'
  import { forgetSent, loadSent, type SentReport } from '../lib/feedback'
  import { t } from '../lib/i18n.svelte'

  let list: SentReport[] = $state(loadSent())
  /** By secret; a secret missing from a read instance is no longer kept. */
  let answers: Record<string, FeedbackAnswer> = $state({})
  /** The instances that answered, and those that could not be reached. */
  let read: string[] = $state([])
  let unreached: string[] = $state([])
  let busy = $state(false)

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

  onMount(() => void refresh())

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
  <div class="settings-group wide" data-feedback-list aria-busy={busy}>
    <h3>{t('feedback.sentTitle')}</h3>
    <p class="settings-help">{t('feedback.sentHelp')}</p>
    <ul>
      {#each list as r (r.secret)}
        {@const answer = answers[r.secret]}
        <li data-feedback-sent-report>
          <div class="line">
            <span class="about">{r.about}</span>
            <span class="state" data-state={answer?.state ?? ''}>{standing(r)}</span>
          </div>
          <span class="settings-help">{when(r.sentAt)} · {r.host}</span>
          {#if answer?.answer}
            <blockquote data-feedback-answer>
              <span class="settings-help">{t('feedback.answer')}</span>
              {answer.answer}
            </blockquote>
          {/if}
          <button class="btn small" onclick={() => forget(r.secret)}>{t('feedback.forget')}</button>
        </li>
      {/each}
    </ul>
    <div class="settings-actions">
      <button class="btn" disabled={busy} onclick={() => void refresh()}
        >{t('feedback.refresh')}</button
      >
    </div>
  </div>
{/if}

<style>
  h3 {
    margin: 0;
    font-size: 14px;
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
