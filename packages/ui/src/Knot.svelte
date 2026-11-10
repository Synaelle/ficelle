<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- The app's knot, drawn as a thread rather than a picture, so it can tie
     itself: `tie` draws the thread and pulls it tight (the notes locking),
     `untie` loosens and fades it (unlocking), `still` just shows it. The
     system's "reduce motion" shows it still. The over-crossing is a real gap
     in the thread, so the knot is transparent on any background. -->
<script lang="ts">
  let {
    motion = 'still',
    size = 88,
    onend,
  }: { motion?: 'still' | 'tie' | 'untie'; size?: number; onend?: () => void } = $props()

  const THREAD =
    'M40 340C140 340 210 320 260 250C300 195 320 130 280 110C240 90 195 120 205 175C215 230 280 300 340 325C390 345 430 340 472 340'
  const OVER = 'M224.2 222C239.8 247.8 262.9 273.4 288.3 293.6'
  const gap = $props.id()
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
  <mask id={gap} maskUnits="userSpaceOnUse" x="-40" y="-70" width="592" height="592">
    <rect x="-40" y="-70" width="592" height="592" fill="white" />
    <path class="cut" d={OVER} />
  </mask>
  <g mask="url(#{gap})"><path class="thread" d={THREAD} pathLength="1" /></g>
  <path class="over" d={OVER} pathLength="1" />
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
  /* the app icon's proportions (brand/app-icon.svg) */
  .thread,
  .over {
    stroke: var(--color-accent);
    stroke-width: 52;
  }
  .cut {
    stroke: black;
    stroke-width: 100;
    stroke-linecap: butt;
  }

  /* One drawn length drives both pieces, so the gap stays cut while the
     thread draws or lets go: --head is how far the thread is drawn, --tail
     how much has let go at the start. The over piece is the part of the
     thread from 0.6673 to 0.7776 of its length and follows it there. */
  @property --head {
    syntax: '<number>';
    inherits: true;
    initial-value: 1;
  }
  @property --tail {
    syntax: '<number>';
    inherits: true;
    initial-value: 0;
  }
  .thread,
  .over {
    stroke-dasharray: 1 2;
  }
  .thread {
    stroke-dashoffset: calc(1 - var(--head) - var(--tail));
  }
  .over {
    stroke-dashoffset: calc(
      1 - clamp(0, (var(--head) - 0.6673) / 0.1103, 1) -
        clamp(0, (var(--tail) - 0.6673) / 0.1103, 1)
    );
  }

  /* tying: the thread draws itself over and under, then a pull */
  .tie {
    animation:
      knot-draw 520ms cubic-bezier(0.55, 0.05, 0.3, 1) forwards,
      knot-pull-done 240ms cubic-bezier(0.3, 1.6, 0.5, 1) 520ms both;
  }

  /* untying: the knot loosens a little, then lets go */
  .untie {
    animation:
      knot-undraw 340ms ease-in forwards,
      knot-loosen-done 340ms cubic-bezier(0.4, 0, 0.6, 1) forwards;
  }

  @keyframes knot-draw {
    from {
      --head: 0;
    }
    to {
      --head: 1;
    }
  }
  @keyframes knot-undraw {
    to {
      --tail: 1;
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
