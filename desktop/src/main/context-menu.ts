import { clipboard, Menu, type BrowserWindow, type ContextMenuParams, type MenuItemConstructorOptions } from 'electron'

// The menu of the operating system (docs/chat.md „Kopieren und Kontextmenü“): wherever the app
// has no menu of its own. The page's own menus (message, chat list) handle the right click
// themselves, and then this event never arrives.

type EditRole = 'undo' | 'redo' | 'cut' | 'copy' | 'paste' | 'selectAll'

export type ContextEntry =
  | { kind: 'separator' }
  | { kind: 'suggestion'; word: string }
  | { kind: 'noSuggestions' }
  | { kind: 'copyLink'; url: string }
  | { kind: 'role'; role: EditRole; label: string }

/** What the click was on: the part of Electron's `params` the menu depends on. */
export type ContextTarget = Pick<
  ContextMenuParams,
  'isEditable' | 'editFlags' | 'misspelledWord' | 'dictionarySuggestions' | 'linkURL' | 'selectionText'
>

/** Blocks of entries with one line between them: none doubled, none at the start or the end. */
function joined(groups: ContextEntry[][]): ContextEntry[] {
  return groups
    .filter((group) => group.length > 0)
    .flatMap((group, i): ContextEntry[] => (i > 0 ? [{ kind: 'separator' }, ...group] : group))
}

/** The entries for a right click, only what is possible there. Empty: no menu. */
export function contextEntries(target: ContextTarget, platform: NodeJS.Platform): ContextEntry[] {
  const mac = platform === 'darwin'
  const flags = target.editFlags
  const role = (role: EditRole, label: string, possible: boolean): ContextEntry[] =>
    possible ? [{ kind: 'role', role, label }] : []
  const copy = role('copy', 'Kopieren', flags.canCopy)

  if (target.isEditable) {
    const spelling: ContextEntry[] = !target.misspelledWord
      ? []
      : target.dictionarySuggestions.length > 0
        ? target.dictionarySuggestions.map((word) => ({ kind: 'suggestion', word }))
        : [{ kind: 'noSuggestions' }]
    return joined([
      spelling,
      [...role('undo', mac ? 'Widerrufen' : 'Rückgängig', flags.canUndo), ...role('redo', 'Wiederholen', flags.canRedo)],
      [...role('cut', 'Ausschneiden', flags.canCut), ...copy, ...role('paste', mac ? 'Einsetzen' : 'Einfügen', flags.canPaste)],
      role('selectAll', 'Alles auswählen', flags.canSelectAll),
    ])
  }

  const link: ContextEntry[] = /^(https?|mailto):/i.test(target.linkURL) ? [{ kind: 'copyLink', url: target.linkURL }] : []
  return [...link, ...(target.selectionText ? copy : [])]
}

export function installContextMenu(win: BrowserWindow): void {
  const contents = win.webContents
  contents.on('context-menu', (_event, params) => {
    const entries = contextEntries(params, process.platform)
    if (entries.length === 0) return
    const template = entries.map((entry): MenuItemConstructorOptions => {
      switch (entry.kind) {
        case 'separator':
          return { type: 'separator' }
        case 'suggestion':
          return { label: entry.word, click: () => contents.replaceMisspelling(entry.word) }
        case 'noSuggestions':
          return { label: 'Keine Vorschläge', enabled: false }
        case 'copyLink':
          return { label: 'Link kopieren', click: () => clipboard.writeText(entry.url) }
        case 'role':
          return { role: entry.role, label: entry.label }
      }
    })
    Menu.buildFromTemplate(template).popup({ window: win, frame: params.frame ?? undefined })
  })
}
