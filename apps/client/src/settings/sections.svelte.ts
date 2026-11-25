// SPDX-License-Identifier: AGPL-3.0-or-later
// Every setting, once: its sections, their groups and rows, where each
// exists and when it shows. The desktop dialog (Settings.svelte) and the
// phone screen (PhoneSettings.svelte) lay the same list out their own way,
// SettingsSection.svelte draws a section's rows for either.

import { HANDHELD, LINUX, MOBILE, WEB } from '../lib/platform'
import { channelChoice, updatesHere } from '../lib/appUpdate.svelte'
import { biometricMethod } from '../lib/biometric'
import {
  applyFont,
  DEFAULT_CODE_FONT,
  FONT_PACKS,
  fontStatus,
  type FontKind,
} from '../lib/chosenFonts.svelte'
import { applyDyslexicFont, OPENDYSLEXIC } from '../lib/dyslexicFont'
import { dyslexicStatus } from '../lib/dyslexicStatus.svelte'
import type { ShownError } from '../lib/errors'
import { LOCALES, t, translatedLine, type Locale } from '../lib/i18n.svelte'
import type { IconName } from '../lib/icons/sets'
import { localSync } from '../lib/localSync.svelte'
import {
  CODE_FONTS,
  experimenting,
  LINE_SPACINGS,
  prefs,
  showTool,
  TEXT_FONTS,
  type CodeFont,
  type TextFont,
} from '../lib/prefs.svelte'
import { quickNoteSupported } from '../lib/quickNote'
import { sync } from '../lib/sync.svelte'
import { MODES } from '../lib/theme.svelte'
import { UI_SIZES } from '../lib/uiSize'
import { TOOL_NAMES } from '../editor/toolNames'
import {
  LOCK_CHOICES,
  lockLabel,
  toolbarState,
  ways,
  type SettingsState,
} from './settingsState.svelte'

export type SectionId =
  | 'appearance'
  | 'accessibility'
  | 'editor'
  | 'language'
  | 'shortcuts'
  | 'security'
  | 'sync'
  | 'data'
  | 'feedback'
  | 'testTools'
  | 'experimental'
  | 'updates'
  | 'about'

interface Base {
  icon: IconName
  /** The translation key of its name. */
  label: string
  /** Whether this device has it at all: a search finds only these. */
  here?: boolean
  /** Whether it shows now, by the other settings or the vault. */
  shown?: () => boolean
  /** What its row says on the phone, for a choice when it is not the picked
   * option's name. */
  value?: () => string
  /** The font it downloads, where that stands: shown under it. */
  load?: () => FontLoading | null
}

export interface SwitchItem extends Base {
  kind: 'switch'
  on: () => boolean
  set: (on: boolean) => void
}

export interface Option {
  label: string
  help?: string
  picked: boolean
  pick: () => void
}

/** How a choice is laid out: a menu, a segmented control, a list of rows
 * under a title, its rows folded under the setting's row, or a page of
 * their own. */
export type ChoiceLook = 'select' | 'segmented' | 'list' | 'fold' | 'page'

export interface ChoiceItem extends Base {
  kind: 'choice'
  options: () => Option[]
  /** On the desktop. */
  wide: ChoiceLook
  /** On the phone. */
  narrow: ChoiceLook
  /** A menu as wide as the others of its kind. */
  width?: 'font-pick' | 'same-width'
  /** One more row under the choices, a button on the desktop. */
  extra?: { label: string; shown: () => boolean; run: () => void }
}

export interface ActionItem extends Base {
  kind: 'action'
  run: () => void
  /** The desktop's button, beside the row's name. */
  button: string
  buttonIcon?: IconName
  primary?: boolean
  /** The desktop row's name, when it is not the button's. */
  wideLabel?: string
  /** Opens a screen of its own: an arrow on the phone's row. */
  opens?: boolean
}

export interface ToolsItem extends Base {
  kind: 'tools'
  switches: () => SwitchItem[]
}

/** Rows each layout draws its own way: what a typed -> becomes, the quick
 * note shortcut, the recovery key and the password. */
export interface OwnItem extends Base {
  kind: 'arrow' | 'quickNote' | 'recovery' | 'password'
}

export type Item = SwitchItem | ChoiceItem | ActionItem | ToolsItem | OwnItem

/** A font downloaded when chosen, where it stands, under its setting. */
export interface FontLoading {
  font: string
  loading: boolean
  error: ShownError | null
  retry: () => void
}

export interface ItemGroup {
  title?: string
  /** The phone's title, when the desktop has none. */
  narrowTitle?: string
  note?: () => string | undefined
  /** On the phone each row is a group of its own. */
  split?: boolean
  items: Item[]
}

export interface Section {
  id: SectionId
  icon: IconName
  /** The phone's icon, when it says more than the section. */
  narrowIcon?: () => IconName
  here: boolean
  /** Where its row sits on the phone's first screen, `inline` for its own
   * rows there instead of a page; none for a desktop section only. */
  home?: 'look' | 'inline' | 'sync' | 'data' | 'more'
  /** What its row on the phone's first screen says. */
  value?: () => string
  groups: ItemGroup[]
}

export const sectionLabel = (id: SectionId) => `settings.${id}`

/** Settings in the pages a section shows from elsewhere, found by their own
 * name in a search. */
const FOUND_INSIDE: (readonly [IconName, string, SectionId])[] = [
  ['backup', 'backups.title', 'data'],
  ['restore', 'backups.restoreFrom', 'data'],
  ['import', 'settings.importTitle', 'data'],
  ['export', 'settings.exportTitle', 'data'],
  ['info', 'settings.credits', 'about'],
  ['search', 'appUpdate.check', 'updates'],
  ...(channelChoice ? ([['flask', 'appUpdate.nightlyVersions', 'updates']] as const) : []),
]

type Prefs = typeof prefs

/** The options of a choice kept in the prefs. */
function stored<K extends keyof Prefs>(
  key: K,
  values: readonly Prefs[K][],
  label: (value: Prefs[K]) => string,
): () => Option[] {
  return () =>
    values.map((value) => ({
      label: label(value),
      picked: prefs[key] === value,
      pick: () => (prefs[key] = value),
    }))
}

function choice(
  icon: IconName,
  label: string,
  wide: ChoiceLook,
  narrow: ChoiceLook,
  options: () => Option[],
  more: Partial<ChoiceItem> = {},
): ChoiceItem {
  return { kind: 'choice', icon, label, wide, narrow, options, ...more }
}

type Flag = { [K in keyof Prefs]: Prefs[K] extends boolean ? K : never }[keyof Prefs]

function flag(key: Flag, icon: IconName, label: string, more: Partial<Base> = {}): SwitchItem {
  return {
    kind: 'switch',
    icon,
    label,
    on: () => prefs[key],
    set: (on) => (prefs[key] = on),
    ...more,
  }
}

const fontLoad = (kind: FontKind, font: TextFont | CodeFont): FontLoading | null =>
  font === 'default'
    ? null
    : { font: FONT_PACKS[font].family, ...fontStatus[kind], retry: () => applyFont(kind, font) }

/** A section that is a page of its own, the same on both. */
const page = (
  id: SectionId,
  icon: IconName,
  here = true,
  more: Partial<Section> = {},
): Section => ({
  id,
  icon,
  here,
  home: 'more',
  groups: [],
  ...more,
})

/** Shown from screenshots and the recent apps or not, where Android can. */
const screens = $state({ hidden: window.AppSystem?.hidden?.() ?? false })

/** The quick note shortcut the system refused: another app holds it. */
export const quickNote = $state({ taken: false })

/** Every section with its settings, for a vault's `settings`. */
export function settingsSections(settings: SettingsState, onlock: () => void): Section[] {
  const vault = () => settings.vault
  const encrypted = () => vault()?.encrypted === true
  /** Only a vault that asks for something to open has anything to lock. */
  const lockable = () => encrypted() && ['guarded', 'pin', 'password'].includes(vault()?.way ?? '')
  const way = (way: string) => t(`settings.way.${way}`, { method: biometricMethod() })
  const all: Section[] = [
    {
      id: 'appearance',
      icon: 'palette',
      home: 'look',
      here: true,
      value: () =>
        prefs.mode === 'dark' && prefs.pureBlack
          ? t('settings.pureBlack')
          : t(`settings.theme_${prefs.mode}`),
      groups: [
        {
          split: true,
          items: [
            choice(
              'eye',
              'settings.theme',
              'segmented',
              'list',
              stored('mode', MODES, (mode) => t(`settings.theme_${mode}`)),
            ),
            // only dark has a black to make deeper
            flag('pureBlack', 'palette', 'settings.pureBlack', {
              shown: () => prefs.mode !== 'light',
            }),
          ],
        },
      ],
    },
    {
      id: 'accessibility',
      icon: 'textSize',
      home: 'look',
      here: true,
      value: () => t(`settings.uiSize_${prefs.uiSize}`),
      groups: [
        {
          split: true,
          items: [
            choice(
              'textSize',
              'settings.uiSize',
              'segmented',
              'list',
              stored('uiSize', UI_SIZES, (size) => t(`settings.uiSize_${size}`)),
            ),
            flag('haptics', 'vibrate', 'settings.haptics', { here: MOBILE }),
            flag('dyslexicFont', 'font', 'settings.dyslexicFont', {
              load: () =>
                prefs.dyslexicFont
                  ? {
                      font: OPENDYSLEXIC.family,
                      ...dyslexicStatus,
                      retry: () => applyDyslexicFont(true),
                    }
                  : null,
            }),
          ],
        },
      ],
    },
    {
      id: 'editor',
      icon: 'textStyle',
      home: 'look',
      here: true,
      groups: [
        {
          items: [
            choice(
              'font',
              'settings.font',
              'select',
              'fold',
              stored('font', TEXT_FONTS, (font) =>
                font === 'default' ? t('settings.fontDefault') : FONT_PACKS[font].family,
              ),
              { width: 'font-pick', load: () => fontLoad('text', prefs.font) },
            ),
            choice(
              'code',
              'settings.codeFont',
              'select',
              'fold',
              stored('codeFont', CODE_FONTS, (font) =>
                font === 'default' ? DEFAULT_CODE_FONT : FONT_PACKS[font].family,
              ),
              { width: 'font-pick', load: () => fontLoad('code', prefs.codeFont) },
            ),
            choice(
              'textStyle',
              'settings.lineSpacing',
              'segmented',
              'fold',
              stored('lineSpacing', LINE_SPACINGS, (spacing) =>
                t(`settings.lineSpacing_${spacing}`),
              ),
            ),
          ],
        },
        {
          items: [
            flag('autoLists', 'list', 'settings.autoLists'),
            flag('emojiShortcodes', 'emoji', 'settings.emojiShortcodes'),
            // a phone spells with its system
            flag('spellcheck', 'pencil', 'settings.spellcheck', { here: !HANDHELD }),
            {
              kind: 'arrow',
              icon: 'right',
              label: 'settings.arrow',
              value: () => prefs.arrow || t('settings.arrowOff'),
            },
          ],
        },
        // how the app behaves, apart from what typing does
        {
          note: () => (quickNote.taken ? t('settings.quickNoteTaken') : undefined),
          items: [
            // a phone never lists beside the note
            flag('openNoteFirst', 'pin', 'settings.openNoteFirst', { here: !HANDHELD }),
            flag('scopedSearch', 'search', 'settings.scopedSearch'),
            flag('openLastNote', 'allNotes', 'settings.openLastNote'),
            flag('nameFromAttachment', 'attachment', 'settings.nameFromAttachment'),
            {
              kind: 'quickNote',
              icon: 'keyboard',
              label: 'settings.quickNote',
              here: quickNoteSupported,
            },
          ],
        },
        // a long list seldom changed: folded under its row
        {
          items: [
            {
              kind: 'tools',
              icon: 'bold',
              label: 'settings.toolbarButtons',
              value: toolbarState,
              switches: () =>
                TOOL_NAMES.map((tool) => ({
                  kind: 'switch',
                  icon: tool.icon,
                  label: tool.label,
                  on: () => !prefs.hiddenTools.includes(tool.icon),
                  set: (on) => showTool(tool.icon, on),
                })),
            },
          ],
        },
      ],
    },
    {
      id: 'language',
      icon: 'language',
      home: 'look',
      here: true,
      value: () =>
        LOCALES.find((l) => l.code === prefs.language)?.name ?? t('settings.languageSystem'),
      groups: [
        {
          split: true,
          items: [
            choice(
              'language',
              'settings.language',
              'select',
              'fold',
              () => [
                {
                  label: t('settings.languageSystem'),
                  picked: prefs.language === null,
                  pick: () => (prefs.language = null),
                },
                ...LOCALES.map((locale) => ({
                  label: locale.name,
                  help: translatedLine(locale.done),
                  picked: prefs.language === locale.code,
                  pick: () => (prefs.language = locale.code as Locale),
                })),
              ],
              { width: 'same-width' },
            ),
            choice(
              'timer',
              'settings.clock',
              'select',
              'fold',
              stored('clock', ['system', '24', '12'], (clock) => t(`settings.clock_${clock}`)),
              { width: 'same-width' },
            ),
          ],
        },
      ],
    },
    // keys to press: the desktop's alone
    page('shortcuts', 'keyboard', true, { home: undefined }),
    {
      id: 'security',
      icon: 'lock',
      home: 'inline',
      here: true,
      groups: [
        {
          narrowTitle: 'settings.phone.groupPrivacy',
          note: () => (vault() && !encrypted() ? t('settings.encryptNote') : undefined),
          items: [
            {
              kind: 'action',
              icon: 'lock',
              label: 'settings.encrypt',
              wideLabel: 'settings.phone.plain',
              button: 'settings.encrypt',
              buttonIcon: 'lock',
              primary: true,
              opens: true,
              shown: () => !!vault() && !encrypted(),
              run: () => settings.encrypt(),
            },
            // a choice only where there is more than the password (not the web version)
            choice(
              'fingerprint',
              'settings.wayTitle',
              'list',
              'page',
              () => {
                const v = vault()
                return (v ? ways(v) : []).map((option) => ({
                  label: way(option),
                  help: option === 'device' ? t('settings.wayHelp.device') : undefined,
                  picked: v?.way === option,
                  pick: () => void settings.setWay(option),
                }))
              },
              {
                shown: () => {
                  const v = vault()
                  return !!v?.encrypted && ways(v).length > 1
                },
                value: () => way(vault()?.way ?? ''),
                extra: {
                  label: 'pin.change',
                  shown: () => vault()?.way === 'pin',
                  run: () => void settings.setWay('pin'),
                },
              },
            ),
            choice(
              'timer',
              'settings.autoLock',
              'select',
              'page',
              stored('autoLockMinutes', LOCK_CHOICES, lockLabel),
              { shown: lockable },
            ),
            {
              kind: 'action',
              icon: 'lock',
              label: 'settings.lockNow',
              button: 'sidebar.lock',
              buttonIcon: 'lock',
              shown: lockable,
              run: onlock,
            },
            {
              kind: 'switch',
              icon: 'eyeOff',
              label: 'settings.phone.hideScreens',
              here: typeof window.AppSystem?.setHidden === 'function',
              on: () => screens.hidden,
              set: (on) => {
                screens.hidden = on
                window.AppSystem?.setHidden?.(on)
              },
            },
          ],
        },
        // an unencrypted vault has no password, recovery key or way to unlock
        {
          title: 'settings.phone.groupPassword',
          note: () => (vault()?.recovery ? t('recovery.has') : t('recovery.noneNow')),
          items: [
            { kind: 'recovery', icon: 'key', label: 'recovery.heading', shown: encrypted },
            {
              kind: 'password',
              icon: 'pencil',
              label: 'settings.changePassword',
              shown: () => encrypted() && !!vault()?.hasPassword,
            },
          ],
        },
      ],
    },
    // the web version cannot reach other devices or a server
    page('sync', 'synced', !WEB, {
      home: 'sync',
      narrowIcon: () => (sync.status.state === 'off' && !localSync.summary ? 'offline' : 'synced'),
      // a device syncs with a server or with its paired devices
      value: () =>
        sync.status.state === 'off' && localSync.summary
          ? t(`local.status.${localSync.summary.key}`, { name: localSync.summary.name })
          : t(`sync.state.${sync.status.state}`),
    }),
    page('data', 'backup', true, { home: 'data' }),
    // the web version has no place to send feedback yet
    page('feedback', 'feedback', !WEB),
    // dev and preview builds only
    page('testTools', 'log', __APP_DEV_TOOLS__),
    // what is still being tried, only where it applies and for those who
    // follow the nightlies
    page('experimental', 'flask', experimenting(), {
      groups: [
        {
          items: [
            flag('evenSpacing', 'list', 'settings.evenSpacing'),
            flag('windowsText', 'textStyle', 'settings.windowsText', { here: LINUX }),
          ],
        },
      ],
    }),
    // release builds only, the web version is always the one served
    page('updates', 'export', updatesHere),
    page('about', 'info', true, { value: () => __APP_VERSION__ }),
  ]
  return all.filter((section) => section.here)
}

/** Whether an item shows now. */
export const showing = (item: Item) => item.here !== false && (item.shown?.() ?? true)

/** The settings inside the sections a search finds by their own name, with
 * their section. The security rows are found on the phone's first screen. */
export function settingsInside(sections: Section[]) {
  return [
    ...sections
      .filter((s) => s.id !== 'security')
      .flatMap((s) =>
        s.groups.flatMap((g) =>
          g.items
            .filter((item) => item.here !== false)
            .map((item) => [item.icon, item.label, s.id] as const),
        ),
      ),
    ...FOUND_INSIDE.filter(([, , id]) => sections.some((s) => s.id === id)),
  ]
}
