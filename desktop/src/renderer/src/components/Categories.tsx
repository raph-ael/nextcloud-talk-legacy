// Page „Kategorien“ (docs/categories.md, desktop only): replaces the chat area like the group
// settings; back arrow and Esc close it, opening a chat closes it too.

import { useCallback, useEffect, useRef, useState } from 'react'
import type { CategoryView } from '@shared/core'
import { closeCategories, useStore } from '../store'
import { toast } from '../call/store'
import { ConfirmDialog, Modal } from './Modal'
import { MessageMenu } from './MessageMenu'
import { Check, Chips, LoadError, PaneHeader, PersonRow, PlaceholderRows, PlusIcon, RoundIcon, useEscape } from './People'

/** A failed call (rejected promise) as the German text of the contract. */
const offline = () => 'Keine Verbindung'

/** „Chats auswählen“ for a new category (`id` null) or an existing one. */
interface Pick {
  id: string | null
  name: string
  initial: string[]
}

type Dialog =
  | { kind: 'new'; name: string }
  | { kind: 'menu'; category: CategoryView; x: number; y: number }
  | { kind: 'rename'; category: CategoryView }
  | { kind: 'delete'; category: CategoryView }

function chatCount(n: number): string {
  return n === 1 ? '1 Chat' : `${n} Chats`
}

/** `onClose`: back arrow and Esc (default: back to the chat; in the settings: to „Chats“). */
export function Categories({ onClose = closeCategories }: { onClose?: () => void } = {}) {
  const rooms = useStore((s) => s.rooms)
  const composing = useStore((s) => s.composing)
  const [list, setList] = useState<CategoryView[] | null>(null)
  const [failed, setFailed] = useState(false)
  const [pick, setPick] = useState<Pick | null>(null)
  const [dialog, setDialog] = useState<Dialog | null>(null)
  const [busy, setBusy] = useState(false)
  const pane = useRef<HTMLElement>(null)

  const load = useCallback(async () => {
    await window.talk.categories().then(
      (l) => {
        setList(l)
        setFailed(false)
      },
      () => setFailed(true),
    )
  }, [])
  // Every change comes back as `roomsChanged`, so a new room list reloads the page.
  useEffect(() => void load(), [load, rooms])

  useEscape(
    pane,
    () => {
      if (busy) return
      if (pick) back()
      else onClose()
    },
    !composing,
  )

  const closeDialog = useCallback(() => {
    if (!busy) setDialog(null)
  }, [busy])

  /** Runs a change; on an error the toast shows it. Resolves to success. */
  async function run(action: () => Promise<string | null>): Promise<boolean> {
    setBusy(true)
    const error = await action().catch(offline)
    setBusy(false)
    if (error) toast(error)
    await load()
    return !error
  }

  function choose(category: CategoryView) {
    const initial = rooms.filter((r) => !r.isArchived && r.tagIds.includes(category.id)).map((r) => r.token)
    setPick({ id: category.id, name: category.name, initial })
  }

  /** From „Chats auswählen“ back: a new category returns to its name. */
  function back() {
    if (pick?.id === null) setDialog({ kind: 'new', name: pick.name })
    setPick(null)
  }

  async function save(tokens: string[]) {
    if (!pick) return
    if (pick.id === null) {
      setBusy(true)
      const res = await window.talk.createCategory(pick.name, tokens).catch(() => ({ id: null, error: offline() }))
      setBusy(false)
      if (res.error || !res.id) {
        toast(res.error ?? 'Kategorie konnte nicht erstellt werden')
        return
      }
      void load()
      setPick(null)
    } else {
      const id = pick.id
      if (await run(() => window.talk.setCategoryRooms(id, tokens))) setPick(null)
    }
  }

  if (pick) {
    return (
      <section className="chat group-info" ref={pane}>
        <ChatPicker
          key={pick.id ?? 'new'}
          initial={pick.initial}
          action={pick.id === null ? 'Erstellen' : 'Speichern'}
          busy={busy}
          onBack={() => !busy && back()}
          onAction={(tokens) => void save(tokens)}
        />
      </section>
    )
  }

  return (
    <section className="chat group-info" ref={pane}>
      <PaneHeader title="Kategorien" onBack={onClose} />
      <div className="settings-scroll">
        <div className="settings">
          <section className="settings-section">
            <button className="person-row" onClick={() => setDialog({ kind: 'new', name: '' })}>
              <RoundIcon>
                <PlusIcon />
              </RoundIcon>
              <span className="person-text">
                <span className="person-name">Neue Kategorie</span>
              </span>
            </button>
          </section>
          <section className="settings-section" aria-label="Kategorien">
            {failed && list === null ? (
              <LoadError text="Konnte nicht geladen werden" retry={() => void load()} />
            ) : list === null ? (
              <PlaceholderRows count={2} />
            ) : (
              list.map((c, i) => (
                <CategoryRow
                  key={c.id}
                  category={c}
                  first={i === 0}
                  last={i === list.length - 1}
                  busy={busy}
                  onOpen={() => choose(c)}
                  onMove={(offset) => (busy ? Promise.resolve(false) : run(() => window.talk.moveCategory(c.id, offset)))}
                  onMenu={(x, y) => setDialog({ kind: 'menu', category: c, x, y })}
                />
              ))
            )}
          </section>
        </div>
      </div>

      {dialog?.kind === 'new' && (
        <NameDialog
          title="Neue Kategorie"
          initial={dialog.name}
          confirm="Weiter"
          busy={false}
          onCancel={() => setDialog(null)}
          onConfirm={(name) => {
            setDialog(null)
            setPick({ id: null, name, initial: [] })
          }}
        />
      )}
      {dialog?.kind === 'menu' && (
        <MessageMenu
          x={dialog.x}
          y={dialog.y}
          onClose={() => setDialog((d) => (d?.kind === 'menu' ? null : d))}
          items={[
            { label: 'Umbenennen', onSelect: () => setDialog({ kind: 'rename', category: dialog.category }) },
            { label: 'Löschen', danger: true, onSelect: () => setDialog({ kind: 'delete', category: dialog.category }) },
          ]}
        />
      )}
      {dialog?.kind === 'rename' && (
        <NameDialog
          title="Kategorie umbenennen"
          initial={dialog.category.name}
          confirm="Speichern"
          busy={busy}
          onCancel={closeDialog}
          onConfirm={async (name) => {
            const c = dialog.category
            if (name === c.name || (await run(() => window.talk.renameCategory(c.id, name)))) setDialog(null)
          }}
        />
      )}
      {dialog?.kind === 'delete' && (
        <ConfirmDialog
          title={`Kategorie „${dialog.category.name}“ löschen?`}
          text="Die Chats bleiben erhalten."
          confirm="Löschen"
          danger
          busy={busy}
          onCancel={closeDialog}
          onConfirm={async () => {
            const c = dialog.category
            if (await run(() => window.talk.deleteCategory(c.id))) setDialog(null)
          }}
        />
      )}
    </section>
  )
}

/** Name, „N Chats“, ↑ ↓ and (own categories) the menu; a click on an own one picks its chats. */
function CategoryRow({
  category,
  first,
  last,
  busy,
  onOpen,
  onMove,
  onMenu,
}: {
  category: CategoryView
  first: boolean
  last: boolean
  busy: boolean
  onOpen: () => void
  onMove: (offset: number) => Promise<boolean>
  onMenu: (x: number, y: number) => void
}) {
  const own = category.kind === 'custom'
  const up = useRef<HTMLButtonElement>(null)
  const down = useRef<HTMLButtonElement>(null)

  /** The focus stays on the arrow (or the other one at an end) while the row moves. */
  async function move(offset: number) {
    await onMove(offset)
    requestAnimationFrame(() => {
      const [same, other] = offset < 0 ? [up.current, down.current] : [down.current, up.current]
      ;(same && !same.disabled ? same : other)?.focus()
    })
  }

  const text = (
    <span className="settings-text">
      <span className="ellipsis">{category.name}</span>
      <span className="person-sub">{chatCount(category.roomCount)}</span>
    </span>
  )
  return (
    <div className="category-row">
      {own ? (
        <button className="settings-row" onClick={onOpen} title="Chats auswählen">
          {text}
        </button>
      ) : (
        <div className="settings-row static">{text}</div>
      )}
      <button
        ref={up}
        className="icon-button"
        aria-label={`${category.name} nach oben`}
        title="Nach oben"
        disabled={first}
        onClick={() => void move(-1)}
      >
        <ArrowIcon up />
      </button>
      <button
        ref={down}
        className="icon-button"
        aria-label={`${category.name} nach unten`}
        title="Nach unten"
        disabled={last}
        onClick={() => void move(1)}
      >
        <ArrowIcon />
      </button>
      {own ? (
        <button
          className="icon-button"
          aria-label={`Mehr zu ${category.name}`}
          title="Mehr"
          aria-haspopup="menu"
          disabled={busy}
          onClick={(e) => {
            const r = e.currentTarget.getBoundingClientRect()
            onMenu(r.right - 180, r.bottom + 4)
          }}
        >
          <svg viewBox="0 0 24 24" width="20" height="20" aria-hidden>
            <circle cx="5" cy="12" r="2" fill="currentColor" />
            <circle cx="12" cy="12" r="2" fill="currentColor" />
            <circle cx="19" cy="12" r="2" fill="currentColor" />
          </svg>
        </button>
      ) : (
        <span className="icon-spacer" aria-hidden />
      )}
    </div>
  )
}

/** Name field for „Neue Kategorie“ and „Umbenennen“ (also the chat menu); the action needs a name. */
export function NameDialog({
  title,
  initial,
  confirm,
  busy,
  onCancel,
  onConfirm,
}: {
  title: string
  initial: string
  confirm: string
  busy: boolean
  onCancel: () => void
  onConfirm: (name: string) => void
}) {
  const [name, setName] = useState(initial)
  const trimmed = name.trim()
  const submit = () => trimmed && !busy && onConfirm(trimmed)
  return (
    <Modal label={title} className="sheet" onClose={() => !busy && onCancel()}>
      <h2 className="dialog-title">{title}</h2>
      <label className="field">
        <span className="field-label">Name der Kategorie</span>
        <input
          className="input"
          maxLength={250}
          value={name}
          disabled={busy}
          data-autofocus
          onChange={(e) => setName(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && submit()}
        />
      </label>
      <div className="dialog-buttons">
        <button className="dialog-btn" onClick={onCancel} disabled={busy}>
          Abbrechen
        </button>
        <button className="dialog-btn primary" onClick={submit} disabled={busy || !trimmed}>
          {busy ? <span className="spinner tiny light" aria-label="Wird gespeichert" /> : confirm}
        </button>
      </div>
    </Modal>
  )
}

/**
 * „Chats auswählen“: search, chips of the chosen chats, all chats that are not archived with
 * a round check mark (like „Mitglieder hinzufügen“). Picking clears the search.
 */
function ChatPicker({
  initial,
  action,
  busy,
  onBack,
  onAction,
}: {
  initial: string[]
  action: string
  busy: boolean
  onBack: () => void
  onAction: (tokens: string[]) => void
}) {
  const rooms = useStore((s) => s.rooms)
  const [selected, setSelected] = useState<string[]>(initial)
  const [query, setQuery] = useState('')
  const field = useRef<HTMLInputElement>(null)
  const chosen = new Set(selected)
  const all = rooms.filter((r) => !r.isArchived)
  const q = query.trim().toLocaleLowerCase('de')
  const shown = q ? all.filter((r) => r.name.toLocaleLowerCase('de').includes(q)) : all
  const chips = selected
    .map((t) => all.find((r) => r.token === t))
    .filter((r) => r !== undefined)
    .map((r) => ({ id: r.token, name: r.name, token: r.token, avatarVersion: r.avatarVersion }))

  function toggle(token: string) {
    setSelected((s) => (s.includes(token) ? s.filter((t) => t !== token) : [...s, token]))
    setQuery('')
    field.current?.focus()
  }

  return (
    <>
      <PaneHeader title="Chats auswählen" onBack={onBack} />
      <div className="picker-pane">
        <div className="search">
          <input
            ref={field}
            className="input search-input"
            placeholder="Chat suchen"
            aria-label="Chat suchen"
            value={query}
            autoFocus
            disabled={busy}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && shown[0]) toggle(shown[0].token)
              if (e.key === 'Backspace' && query === '' && selected.length > 0) setSelected((s) => s.slice(0, -1))
            }}
          />
        </div>
        <Chips items={chips} onRemove={toggle} />
        <div className="people-list">
          {shown.length === 0 ? (
            <p className="muted center pad">{q ? 'Keine Treffer' : 'Noch keine Chats'}</p>
          ) : (
            shown.map((r) => (
              <PersonRow
                key={r.token}
                name={r.name}
                token={r.token}
                avatarVersion={r.avatarVersion}
                label={`${r.name}, ${chosen.has(r.token) ? 'ausgewählt' : 'nicht ausgewählt'}`}
                onClick={() => toggle(r.token)}
                disabled={busy}
                trailing={<Check on={chosen.has(r.token)} />}
              />
            ))
          )}
        </div>
        <footer className="pane-footer">
          <button className="button-primary" onClick={() => onAction(selected)} disabled={busy}>
            {busy ? <span className="spinner tiny light" aria-label="Bitte warten" /> : action}
          </button>
        </footer>
      </div>
    </>
  )
}

function ArrowIcon({ up = false }: { up?: boolean }) {
  return (
    <svg viewBox="0 0 24 24" width="18" height="18" aria-hidden fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
      <path d={up ? 'M12 19V5M6 11l6-6 6 6' : 'M12 5v14M6 13l6 6 6-6'} />
    </svg>
  )
}
