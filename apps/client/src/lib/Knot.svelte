<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- The app's knot, drawn as a thread rather than a picture, so it can tie
     itself: `tie` draws the thread and pulls it tight (the notes locking),
     `untie` loosens and fades it (unlocking), `still` just shows it. The
     system's "reduce motion" shows it still. The over-crossing is cut with
     the colour behind it, `--knot-behind`. -->
<script lang="ts">
  let {
    motion = 'still',
    size = 88,
    onend,
  }: { motion?: 'still' | 'tie' | 'untie'; size?: number; onend?: () => void } = $props()

  const THREAD =
    'M40 340C140 340 210 320 260 250C300 195 320 130 280 110C240 90 195 120 205 175C215 230 280 300 340 325C390 345 430 340 472 340'
  const OVER = 'M224.2 222C239.8 247.8 262.9 273.4 288.3 293.6'
</script>

<svg
  class="knot {motion}"
  width={size}
  height={size}
  viewBox="0 -30 512 512"
  aria-hidden="true"
  onanimationend={(e) => {
    // the last of each motion's animations says it is over
    if (e.animationName.endsWith('-done')) onend?.()
  }}
>
  <path class="thread" d={THREAD} pathLength="1" />
  <path class="cut" d={OVER} />
  <path class="over" d={OVER} />
</svg>

<style>
  .knot {
    display: block;
    overflow: visible;
  }
  path {
    fill: none;
    stroke-linecap: round;
  }
  .thread,
  .over {
    stroke: var(--color-accent);
    stroke-width: 40;
  }
  .cut {
    stroke: var(--knot-behind, var(--color-surface));
    stroke-width: 84;
    stroke-linecap: butt;
  }

  /* tying: the thread draws itself, the crossing goes over, then a pull */
  .tie .thread {
    stroke-dasharray: 1;
    stroke-dashoffset: 1;
    animation: knot-draw 520ms cubic-bezier(0.55, 0.05, 0.3, 1) forwards;
  }
  .tie .cut,
  .tie .over {
    opacity: 0;
    animation: knot-show 90ms ease-out 430ms forwards;
  }
  .tie {
    animation: knot-pull-done 240ms cubic-bezier(0.3, 1.6, 0.5, 1) 520ms both;
  }

  /* untying: the knot loosens a little, then lets go */
  .untie {
    animation: knot-loosen-done 340ms cubic-bezier(0.4, 0, 0.6, 1) forwards;
  }
  .untie .thread {
    stroke-dasharray: 1;
    animation: knot-undraw 340ms ease-in forwards;
  }

  @keyframes knot-draw {
    to {
      stroke-dashoffset: 0;
    }
  }
  @keyframes knot-show {
    to {
      opacity: 1;
    }
  }
  @keyframes knot-pull-done {
    from {
      transform: scale(1.06);
    }
    to {
      transform: scale(1);
    }
  }
  @keyframes knot-loosen-done {
    40% {
      transform: scale(1.08) rotate(-4deg);
      opacity: 1;
    }
    to {
      transform: scale(0.9) rotate(6deg);
      opacity: 0;
    }
  }
  @keyframes knot-undraw {
    to {
      stroke-dashoffset: -1;
    }
  }

  .knot {
    transform-origin: 50% 50%;
  }

  @media (prefers-reduced-motion: reduce) {
    .tie,
    .tie *,
    .untie,
    .untie * {
      animation-duration: 1ms !important;
      animation-delay: 0ms !important;
    }
  }
</style>
