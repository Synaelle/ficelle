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
  // the thread's second pass, from the bottom of the loop on, drawn again
  // over the cut thread
  const SECOND = 'M205 175C215 230 280 300 340 325C390 345 430 340 472 340'
  // the part of it that crosses the first pass, where the gap is cut
  const CROSSING = 'M218.9 212.8C234.3 241.7 259.8 271 288.3 293.6'
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
    <path class="cut" d={CROSSING} pathLength="1" />
    <path class="cut-end" d={CROSSING} pathLength="1" />
  </mask>
  <g mask="url(#{gap})"><path class="thread" d={THREAD} pathLength="1" /></g>
  <path class="over" d={SECOND} pathLength="1" />
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
  .cut,
  .cut-end {
    stroke: black;
    stroke-width: 100;
  }
  .cut {
    stroke-linecap: butt;
  }

  /* One drawn length drives every piece: --head is how far the thread is
     drawn, --tail how much has let go at the start. The second pass starts
     at 0.6089 of the thread's length and follows it, the crossing runs from
     0.6552 to 0.7776. The gap is cut behind the thread's end as it crosses,
     with a round end around its tip, so it opens as an even margin around
     the strand and the first pass stays whole until the strand reaches it. */
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
  .over,
  .cut {
    stroke-dasharray: 1 2;
  }
  /* the offsets are lengths: Firefox drops a calc() that gives a bare
     number, and the knot is then drawn whole. With pathLength 1 a px is
     one unit of the path */
  .thread {
    stroke-dashoffset: calc((1 - var(--head) - var(--tail)) * 1px);
  }
  .over {
    stroke-dashoffset: calc(
      (
          1 - clamp(0, (var(--head) - 0.6089) / 0.3911, 1) -
            clamp(0, (var(--tail) - 0.6089) / 0.3911, 1)
        ) *
        1px
    );
  }
  .cut {
    stroke-dashoffset: calc((1 - clamp(0, (var(--head) - 0.6552) / 0.1224, 1)) * 1px);
  }
  /* a dot at the thread's end, there only while it crosses */
  .cut-end {
    stroke-dasharray: 0 2;
    stroke-dashoffset: calc(-1px * clamp(0, (var(--head) - 0.6552) / 0.1224, 1));
    stroke-opacity: calc(
      clamp(0, (var(--head) - 0.6552) * 10000, 1) * clamp(0, (0.7776 - var(--head)) * 10000, 1)
    );
  }

  /* tying: the thread draws itself over and under, then a pull */
  .tie {
    animation:
      knot-draw 676ms cubic-bezier(0.55, 0.05, 0.3, 1) forwards,
      knot-pull-done 312ms cubic-bezier(0.3, 1.6, 0.5, 1) 676ms both;
  }

  /* untying: the knot loosens a little, then lets go */
  .untie {
    animation:
      knot-undraw 442ms ease-in forwards,
      knot-loosen-done 442ms cubic-bezier(0.4, 0, 0.6, 1) forwards;
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
