<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- The phone's camera, to read the QR code a sync server's /link page
     shows. Each frame goes to the app as a small greyscale copy, read there
     (commands/qr.rs), so no Google Play services are needed. Android asks
     for the camera the first time, the camera stops as soon as a code is
     read, or the screen closes. -->
<script lang="ts">
  import { brand } from '@app/brand'
  import { invoke } from '@tauri-apps/api/core'
  import { onDestroy, onMount } from 'svelte'
  import { onBack } from '../lib/back'
  import { t } from '../lib/i18n.svelte'
  import { parseOffer } from '../lib/linkOffer'

  let { found, close }: { found: (server: string, offer: string) => void; close: () => void } =
    $props()

  /** The longest side of the copy sent to the reader, enough for a code
   * that fills part of the view. */
  const SIDE = 480

  let video: HTMLVideoElement | undefined = $state()
  let stream: MediaStream | null = null
  let stopped = false
  let timer: ReturnType<typeof setTimeout> | undefined
  let problem = $state('')
  const canvas = document.createElement('canvas')

  $effect(() => onBack(close))

  onMount(async () => {
    try {
      stream = await navigator.mediaDevices.getUserMedia({
        video: { facingMode: { ideal: 'environment' } },
        audio: false,
      })
    } catch {
      problem = t('sync.scanNoCamera', { appName: brand.displayName })
      return
    }
    if (stopped || !video) return stop()
    video.srcObject = stream
    await video.play().catch(() => {})
    timer = setTimeout(look, 300)
  })

  onDestroy(stop)

  function stop() {
    stopped = true
    clearTimeout(timer)
    for (const track of stream?.getTracks() ?? []) track.stop()
    stream = null
  }

  /** One frame to the reader, then the next. */
  async function look() {
    if (stopped || !video) return
    const scale = Math.min(1, SIDE / Math.max(video.videoWidth, video.videoHeight, 1))
    const width = Math.round(video.videoWidth * scale)
    const height = Math.round(video.videoHeight * scale)
    if (width && height) {
      canvas.width = width
      canvas.height = height
      const context = canvas.getContext('2d', { willReadFrequently: true })
      context?.drawImage(video, 0, 0, width, height)
      const rgba = context?.getImageData(0, 0, width, height).data
      if (rgba) {
        const text = await invoke<string | null>('qr_read', {
          width,
          height,
          luma: base64(grey(rgba)),
        }).catch(() => null)
        if (stopped) return
        if (text) {
          const offer = parseOffer(text)
          if (offer) {
            stop()
            found(offer.server, offer.offer)
            return
          }
          problem = t('sync.scanNotOurs', { appName: brand.displayName })
        }
      }
    }
    timer = setTimeout(look, 200)
  }

  function grey(rgba: Uint8ClampedArray): Uint8Array {
    const luma = new Uint8Array(rgba.length / 4)
    for (let i = 0; i < luma.length; i++) {
      const [r = 0, g = 0, b = 0] = rgba.subarray(i * 4, i * 4 + 3)
      luma[i] = (r * 77 + g * 150 + b * 29) >> 8
    }
    return luma
  }

  function base64(bytes: Uint8Array): string {
    let binary = ''
    for (let i = 0; i < bytes.length; i += 0x8000) {
      binary += String.fromCharCode(...bytes.subarray(i, i + 0x8000))
    }
    return btoa(binary)
  }
</script>

<div class="scan" role="dialog" aria-modal="true" aria-label={t('sync.scanTitle')}>
  <h2>{t('sync.scanTitle')}</h2>
  <div class="view">
    <video bind:this={video} playsinline muted></video>
    <div class="frame" aria-hidden="true"></div>
  </div>
  {#if problem}<p class="problem" role="alert">{problem}</p>{/if}
  <button class="btn" type="button" onclick={close}>{t('common.cancel')}</button>
</div>

<style>
  .scan {
    position: fixed;
    inset: 0;
    z-index: 50;
    display: grid;
    grid-template-rows: auto 1fr auto auto;
    gap: var(--space-3);
    padding: calc(var(--space-4) + env(safe-area-inset-top)) var(--space-4)
      calc(var(--space-4) + env(safe-area-inset-bottom));
    background: var(--color-bg);
  }
  h2 {
    margin: 0;
    font-size: var(--title-dialog);
    text-align: center;
  }
  .view {
    position: relative;
    overflow: hidden;
    border-radius: var(--radius-window);
    background: #000;
  }
  video {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
  .frame {
    position: absolute;
    inset: 50% auto auto 50%;
    width: min(64vw, 280px);
    aspect-ratio: 1;
    transform: translate(-50%, -50%);
    border: 2px solid #fff;
    border-radius: var(--radius-control);
    box-shadow: 0 0 0 100vmax rgb(0 0 0 / 0.35);
  }
  .problem {
    margin: 0;
    color: var(--color-danger);
    text-align: center;
  }
  .btn {
    justify-self: center;
  }
</style>
