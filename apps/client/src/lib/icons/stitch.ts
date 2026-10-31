// SPDX-License-Identifier: AGPL-3.0-or-later
// Stitch icons, drawn for the Carbon theme.
// 24 unit grid, 1.75 stroke, round caps. Paths with class st are stitched
// (dashed), class gap cuts a crossing in the background colour, class dot is
// filled. Icons not drawn here fall back to Lucide.

export const stitch: Record<string, string> = {
  newNote:
    '<path d="M13.5 3.5H7a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V9l-5.5-5.5V9H19"/><path d="M12 12.5v5M9.5 15h5"/>',
  folder:
    '<path d="M3.5 17.5v-11a2 2 0 0 1 2-2h3.8l2.2 2.5h7a2 2 0 0 1 2 2v8.5a2 2 0 0 1-2 2h-13a2 2 0 0 1-2-2Z"/><path class="st" d="M6.5 10.5h11"/>',
  tag: '<path d="M11 4H5a1 1 0 0 0-1 1v6l8.6 8.6a1.4 1.4 0 0 0 2 0l5-5a1.4 1.4 0 0 0 0-2Z"/><circle cx="8.5" cy="8.5" r="1.4"/><path class="st" d="M7.6 7.5C6.4 5.6 4.6 3.6 2.5 3"/>',
  pin: '<circle cx="15" cy="9" r="4"/><path d="M12.2 11.8 4.5 19.5"/><path class="st" d="M18.5 15.5c1.5 1.5 1.5 3.5 0 5"/>',
  search:
    '<circle cx="10.5" cy="10.5" r="6"/><path d="m15 15 5.5 5.5"/><path class="st" d="M7.6 10.5a2.9 2.9 0 0 1 2.9-2.9"/>',
  share:
    '<path d="M8.5 10H6.5a2 2 0 0 0-2 2v7a2 2 0 0 0 2 2h11a2 2 0 0 0 2-2v-7a2 2 0 0 0-2-2h-2"/><path d="M12 15.5V3.5M8.5 7 12 3.5 15.5 7"/>',
  synced:
    '<path d="M7 19h10a4 4 0 0 0 .6-7.95A6 6 0 0 0 6.2 10.1 4.5 4.5 0 0 0 7 19Z"/><path d="m9.3 14.6 2 2 3.6-3.6"/>',
  syncing:
    '<path class="st" d="M7 19h10a4 4 0 0 0 .6-7.95A6 6 0 0 0 6.2 10.1 4.5 4.5 0 0 0 7 19Z"/><path d="M12 17v-5M9.8 14.2 12 12l2.2 2.2"/>',
  offline:
    '<path d="M9 19h8a4 4 0 0 0 1.9-.5M20.6 15.4A4 4 0 0 0 17.6 11 6 6 0 0 0 9.5 6.1M6.6 9.3a4.5 4.5 0 0 0 .4 9.7"/><path d="m3.5 3.5 17 17"/>',
  syncError:
    '<path d="M7 19h10a4 4 0 0 0 .6-7.95A6 6 0 0 0 6.2 10.1 4.5 4.5 0 0 0 7 19Z"/><path d="M12 11.6v3"/><circle class="dot" cx="12" cy="16.9" r="1.05"/>',
  settings:
    '<path d="M3.5 6.5h8M17.5 6.5h3M3.5 17.5h3M12.5 17.5h8"/><circle cx="14.5" cy="6.5" r="2.5"/><circle cx="9.5" cy="17.5" r="2.5"/><path class="st" d="M3.5 12h17"/>',
  attachment:
    '<path d="m19.5 11.5-7.3 7.3a5 5 0 0 1-7.1-7.1l8.2-8.2a3.3 3.3 0 0 1 4.7 4.7l-8.1 8.1a1.7 1.7 0 0 1-2.4-2.4l7.3-7.3"/>',
  image:
    '<rect x="3.5" y="4.5" width="17" height="15" rx="2"/><path d="m4 17 5-5 4.5 4.5 2.5-2.5 4 4"/><circle cx="15.5" cy="9" r="1.5"/>',
  lock: '<rect x="5" y="10.5" width="14" height="10" rx="2"/><path d="M8 10.5v-3a4 4 0 0 1 8 0v3"/><path class="st" d="M12 14v3.5"/>',
  trash:
    '<path d="M4 6.5h16M9.5 6.5v-2h5v2M6 6.5l1 12.6a1.5 1.5 0 0 0 1.5 1.4h7a1.5 1.5 0 0 0 1.5-1.4L18 6.5"/><path class="st" d="M10 10.5v6.5M14 10.5v6.5"/>',
  knot: '<path d="M1.5 16.65C6.36 16.65 9.76 15.68 12.19 12.28C14.14 9.6 15.11 6.44 13.17 5.47C11.22 4.5 9.03 5.96 9.52 8.63C10.01 11.31 13.17 14.71 16.08 15.92C18.51 16.9 20.46 16.65 22.5 16.65"/><path class="gap" stroke-linecap="butt" d="M10.45 10.92C11.21 12.17 12.33 13.42 13.57 14.4"/><path d="M10.45 10.92C11.21 12.17 12.33 13.42 13.57 14.4"/>',
  back: '<path d="M15 5.5 8.5 12l6.5 6.5"/>',
  down: '<path d="m6.5 9.5 5.5 5.5 5.5-5.5"/>',
  more: '<circle class="dot" cx="6" cy="12" r="1.4"/><circle class="dot" cx="12" cy="12" r="1.4"/><circle class="dot" cx="18" cy="12" r="1.4"/>',
  check: '<path d="m5 12.5 4.5 4.5L19 7.5"/>',
  move: '<path d="M3.5 17.5v-11a2 2 0 0 1 2-2h3.8l2.2 2.5h7a2 2 0 0 1 2 2v8.5a2 2 0 0 1-2 2h-13a2 2 0 0 1-2-2Z"/><path d="M8.5 13.5h7M13 11l2.5 2.5L13 16"/>',
  list: '<path d="M9 6.5h11M9 12h11M9 17.5h11"/><circle class="dot" cx="4.8" cy="6.5" r="1.2"/><circle class="dot" cx="4.8" cy="12" r="1.2"/><circle class="dot" cx="4.8" cy="17.5" r="1.2"/>',
  tasks:
    '<rect x="3.5" y="4" width="6" height="6" rx="1.2"/><path d="m5 7 1.3 1.3L8.6 6"/><rect x="3.5" y="14" width="6" height="6" rx="1.2"/><path d="M13 7h7.5M13 17h7.5"/>',
  link: '<path d="M10 14a4 4 0 0 0 5.7 0l3.6-3.6a4 4 0 0 0-5.7-5.7l-1.1 1.1"/><path d="M14 10a4 4 0 0 0-5.7 0l-3.6 3.6a4 4 0 0 0 5.7 5.7l1.1-1.1"/>',
  code: '<path d="M8 7.5 3.5 12 8 16.5M16 7.5l4.5 4.5-4.5 4.5"/><path class="st" d="m13.5 5.5-3 13"/>',
  history:
    '<path d="M4.5 12a7.5 7.5 0 1 0 2.2-5.3L4.5 9"/><path d="M4.5 4.5V9H9"/><path class="st" d="M12 8v4l3 2"/>',
  language:
    '<path d="M3.5 18.5 8 6.5l4.5 12M5.2 14.5h5.6"/><path d="M14.5 9.5h6M17.5 7.5v2c0 3.5-1.5 6-3.5 7.5M16 12.5c1 2 2.5 3.5 4.5 4.5"/>',
  info: '<circle cx="12" cy="12" r="8.5"/><path d="M12 11v5.5"/><circle class="dot" cx="12" cy="7.9" r="1.05"/>',
  layoutInline:
    '<path d="M3.5 5.5h17M3.5 18.5h17"/><rect x="3.5" y="9" width="7" height="6" rx="1"/><path class="st" d="M13.5 15h7"/>',
  layoutLeft:
    '<rect x="3.5" y="5" width="8" height="8" rx="1"/><path d="M14.5 6h6M14.5 10h6M14.5 14h6M3.5 18.5h17"/>',
  layoutRight:
    '<rect x="12.5" y="5" width="8" height="8" rx="1"/><path d="M3.5 6h6M3.5 10h6M3.5 14h6M3.5 18.5h17"/>',
  layoutCenter:
    '<path d="M3.5 4.5h17M3.5 19.5h17"/><rect x="7" y="8" width="10" height="8" rx="1"/>',
  layoutFull:
    '<path d="M3.5 4.5h17M3.5 19.5h17"/><rect x="3.5" y="8" width="17" height="8" rx="1"/>',
}
