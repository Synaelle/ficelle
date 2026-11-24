// SPDX-License-Identifier: AGPL-3.0-or-later
// Fonts tried in place of the theme's, for the interface and the notes, while
// the app's font is being chosen. Each one's files load only once picked.

export const TRIAL_FONTS = ['current', 'ubuntu', 'roboto', 'lato', 'firaSans', 'rubik'] as const
export type TrialFont = (typeof TRIAL_FONTS)[number]

export const TRIAL_FONT_NAMES: Record<Exclude<TrialFont, 'current'>, string> = {
  ubuntu: 'Ubuntu',
  roboto: 'Roboto',
  lato: 'Lato',
  firaSans: 'Fira Sans',
  rubik: 'Rubik',
}

const FAMILIES: Record<Exclude<TrialFont, 'current'>, string> = {
  ubuntu: 'Ubuntu',
  roboto: 'Roboto Variable',
  lato: 'Lato',
  firaSans: 'Fira Sans',
  rubik: 'Rubik Variable',
}

const LOADS: Record<Exclude<TrialFont, 'current'>, () => Promise<unknown>> = {
  ubuntu: () =>
    Promise.all([
      import('@fontsource/ubuntu/400.css'),
      import('@fontsource/ubuntu/400-italic.css'),
      import('@fontsource/ubuntu/500.css'),
      import('@fontsource/ubuntu/700.css'),
      import('@fontsource/ubuntu/700-italic.css'),
    ]),
  roboto: () =>
    Promise.all([
      import('@fontsource-variable/roboto/wght.css'),
      import('@fontsource-variable/roboto/wght-italic.css'),
    ]),
  lato: () =>
    Promise.all([
      import('@fontsource/lato/400.css'),
      import('@fontsource/lato/400-italic.css'),
      import('@fontsource/lato/700.css'),
      import('@fontsource/lato/700-italic.css'),
    ]),
  firaSans: () =>
    Promise.all([
      import('@fontsource/fira-sans/400.css'),
      import('@fontsource/fira-sans/400-italic.css'),
      import('@fontsource/fira-sans/500.css'),
      import('@fontsource/fira-sans/600.css'),
      import('@fontsource/fira-sans/700.css'),
      import('@fontsource/fira-sans/700-italic.css'),
    ]),
  rubik: () =>
    Promise.all([
      import('@fontsource-variable/rubik/wght.css'),
      import('@fontsource-variable/rubik/wght-italic.css'),
    ]),
}

/** The family to set for `font`, its files asked for, or null for the theme's own. */
export function trialFamily(font: TrialFont): string | null {
  if (font === 'current') return null
  void LOADS[font]()
  return FAMILIES[font]
}

/** Code fonts tried the same way, for the families above that have one. */
export const TRIAL_MONOS = ['current', 'robotoMono', 'firaMono'] as const
export type TrialMono = (typeof TRIAL_MONOS)[number]

export const TRIAL_MONO_NAMES: Record<Exclude<TrialMono, 'current'>, string> = {
  robotoMono: 'Roboto Mono',
  firaMono: 'Fira Mono',
}

const MONO_FAMILIES: Record<Exclude<TrialMono, 'current'>, string> = {
  robotoMono: 'Roboto Mono Variable',
  firaMono: 'Fira Mono',
}

const MONO_LOADS: Record<Exclude<TrialMono, 'current'>, () => Promise<unknown>> = {
  robotoMono: () =>
    Promise.all([
      import('@fontsource-variable/roboto-mono/wght.css'),
      import('@fontsource-variable/roboto-mono/wght-italic.css'),
    ]),
  firaMono: () =>
    Promise.all([
      import('@fontsource/fira-mono/400.css'),
      import('@fontsource/fira-mono/500.css'),
      import('@fontsource/fira-mono/700.css'),
    ]),
}

/** The code font family to set for `font`, its files asked for, or null for the theme's own. */
export function trialMonoFamily(font: TrialMono): string | null {
  if (font === 'current') return null
  void MONO_LOADS[font]()
  return MONO_FAMILIES[font]
}
