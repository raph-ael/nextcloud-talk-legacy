// Building blocks shared by „Neue Nachricht“, „Mitglieder hinzufügen“ and the group settings
// (docs/groups.md): user search, person rows, round check marks, chips, the pane header.

import { useCallback, useEffect, useRef, useState, type ReactNode, type RefObject } from 'react'
import type { UserView } from '@shared/core'
import { Avatar } from './Avatar'

/** `searchUsers` with 250 ms delay; `users` is null while loading. */
export function useUserSearch(query: string, excludeToken: string | null) {
  const [users, setUsers] = useState<UserView[] | null>(null)
  const [failed, setFailed] = useState(false)
  const [attempt, setAttempt] = useState(0)
  useEffect(() => {
    let cancelled = false
    setUsers(null)
    setFailed(false)
    const timer = setTimeout(
      () => {
        window.talk
          .searchUsers(query.trim(), excludeToken)
          .then((list) => !cancelled && setUsers(list))
          .catch(() => !cancelled && setFailed(true))
      },
      attempt === 0 && query.trim() === '' ? 0 : 250,
    )
    return () => {
      cancelled = true
      clearTimeout(timer)
    }
  }, [query, excludeToken, attempt])
  const retry = useCallback(() => setAttempt((n) => n + 1), [])
  return { users, failed, retry }
}

/** Placeholders, „Keine Treffer“, the error with „Erneut versuchen“, or the rows. */
export function UserResults({
  users,
  failed,
  retry,
  errorText = 'Keine Verbindung',
  row,
}: {
  users: UserView[] | null
  failed: boolean
  retry: () => void
  errorText?: string
  row: (u: UserView) => ReactNode
}) {
  if (failed) return <LoadError text={errorText} retry={retry} />
  if (users === null) return <PlaceholderRows />
  if (users.length === 0) return <p className="muted center pad">Keine Treffer</p>
  return <>{users.map(row)}</>
}

export function LoadError({ text, retry }: { text: string; retry: () => void }) {
  return (
    <div className="load-error">
      <p className="muted">{text}</p>
      <button className="pill-btn" onClick={retry}>
        Erneut versuchen
      </button>
    </div>
  )
}

export function PlaceholderRows({ count = 3 }: { count?: number }) {
  return (
    <>
      {Array.from({ length: count }, (_, i) => (
        <div key={i} className="person-row placeholder" aria-hidden>
          <span className="ph-avatar" />
          <span className="ph-line" />
        </div>
      ))}
    </>
  )
}

/** A person (or with `token` a chat): avatar 40, name and a muted second line; `trailing` on the right. */
export function PersonRow({
  name,
  userId,
  token,
  avatarVersion,
  subline,
  trailing,
  onClick,
  label,
  disabled,
}: {
  name: string
  userId?: string
  token?: string
  avatarVersion?: string
  subline?: string
  trailing?: ReactNode
  onClick?: () => void
  label?: string
  disabled?: boolean
}) {
  const body = (
    <>
      <Avatar name={name} userId={userId} token={token} version={avatarVersion} size={40} />
      <span className="person-text">
        <span className="person-name">{name}</span>
        {subline && <span className="person-sub">{subline}</span>}
      </span>
      {trailing}
    </>
  )
  return onClick ? (
    <button className="person-row" onClick={onClick} aria-label={label} disabled={disabled}>
      {body}
    </button>
  ) : (
    <div className="person-row static" aria-label={label}>
      {body}
    </div>
  )
}

/** Round check mark: empty circle, or filled in accent with a white tick. */
export function Check({ on }: { on: boolean }) {
  return (
    <span className={`check${on ? ' on' : ''}`} aria-hidden>
      {on && (
        <svg viewBox="0 0 24 24" width="16" height="16">
          <path fill="none" stroke="currentColor" strokeWidth="2.6" strokeLinecap="round" strokeLinejoin="round" d="M6 12.5l4 4 8-9" />
        </svg>
      )}
    </span>
  )
}

/** Accent circle with an icon (rows „Neue Gruppe“, „Mitglieder hinzufügen“). */
export function RoundIcon({ children }: { children: ReactNode }) {
  return (
    <span className="round-icon" aria-hidden>
      {children}
    </span>
  )
}

/** Something chosen: a person (`id` is the user id) or, with `token`, a chat. */
export interface ChipItem {
  id: string
  name: string
  token?: string
  avatarVersion?: string
}

export function Chips({ items, onRemove }: { items: ChipItem[]; onRemove: (id: string) => void }) {
  if (items.length === 0) return null
  return (
    <div className="chips">
      {items.map((c) => (
        <button key={c.id} className="chip" onClick={() => onRemove(c.id)} aria-label={`${c.name} entfernen`} title={c.name}>
          <Avatar name={c.name} userId={c.token ? undefined : c.id} token={c.token} version={c.avatarVersion} size={24} />
          <span className="ellipsis">{c.token ? c.name : c.name.split(' ')[0]}</span>
          <svg viewBox="0 0 24 24" width="14" height="14" aria-hidden>
            <path stroke="currentColor" strokeWidth="2" strokeLinecap="round" d="M7 7l10 10M17 7L7 17" />
          </svg>
        </button>
      ))}
    </div>
  )
}

/** Title bar of a step with the back arrow. */
export function PaneHeader({ title, onBack, backLabel = 'Zurück', children }: {
  title: string
  onBack: () => void
  backLabel?: string
  children?: ReactNode
}) {
  return (
    <header className="pane-header">
      <button className="icon-button" onClick={onBack} aria-label={backLabel} title={backLabel}>
        <svg viewBox="0 0 24 24" width="20" height="20" aria-hidden>
          <path fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" d="M15 5l-7 7 7 7" />
        </svg>
      </button>
      <h2>{title}</h2>
      {children}
    </header>
  )
}

/**
 * Esc goes one step back while the focus is inside `container` (or nowhere). Modals and
 * menus handle Esc in the capture phase and stop it, so they come first.
 */
export function useEscape(container: RefObject<HTMLElement | null>, onEscape: () => void, allowBody = true) {
  const handler = useRef(onEscape)
  handler.current = onEscape
  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if (e.key !== 'Escape' || e.defaultPrevented) return
      const active = document.activeElement
      const inside = !!container.current && !!active && container.current.contains(active)
      if (!inside && !(allowBody && (active === document.body || active === null))) return
      e.preventDefault()
      handler.current()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [container, allowBody])
}

/** Hidden file input for a picture; `pick()` opens the system dialog. */
export function usePhotoInput(onFile: (file: File) => void) {
  const input = useRef<HTMLInputElement>(null)
  const element = (
    <input
      ref={input}
      type="file"
      accept="image/*"
      hidden
      onChange={(e) => {
        const file = e.target.files?.[0]
        e.target.value = ''
        if (file) onFile(file)
      }}
    />
  )
  return { element, pick: () => input.current?.click() }
}

/** A chosen picture: bytes for the core and an object URL for the preview. */
export interface Photo {
  bytes: Uint8Array
  url: string
}

export async function readPhoto(file: File): Promise<Photo> {
  return { bytes: new Uint8Array(await file.arrayBuffer()), url: URL.createObjectURL(file) }
}

/** Frees the preview URL of the previous picture whenever it changes. */
export function useRevokeOnChange(photo: Photo | null | undefined) {
  useEffect(() => {
    if (!photo) return
    return () => URL.revokeObjectURL(photo.url)
  }, [photo])
}

export function CameraIcon({ size = 28 }: { size?: number }) {
  return (
    <svg viewBox="0 0 24 24" width={size} height={size} aria-hidden>
      <path
        fill="none"
        stroke="currentColor"
        strokeWidth="1.7"
        strokeLinejoin="round"
        d="M4 8h3l1.5-2h7L17 8h3v11H4z"
      />
      <circle cx="12" cy="13" r="3.5" fill="none" stroke="currentColor" strokeWidth="1.7" />
    </svg>
  )
}

export function PeopleIcon({ size = 22 }: { size?: number }) {
  return (
    <svg viewBox="0 0 24 24" width={size} height={size} aria-hidden>
      <circle cx="9" cy="8.5" r="3.2" fill="none" stroke="currentColor" strokeWidth="1.8" />
      <path fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" d="M3 19c.6-3.3 3-5 6-5s5.4 1.7 6 5" />
      <circle cx="16.5" cy="9" r="2.6" fill="none" stroke="currentColor" strokeWidth="1.6" />
      <path fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" d="M16.5 14c2.4 0 4 1.4 4.5 4" />
    </svg>
  )
}

export function PlusIcon({ size = 22 }: { size?: number }) {
  return (
    <svg viewBox="0 0 24 24" width={size} height={size} aria-hidden>
      <path stroke="currentColor" strokeWidth="2" strokeLinecap="round" d="M12 5v14M5 12h14" />
    </svg>
  )
}

/**
 * Step „Mitglieder hinzufügen“: search, chips of the chosen people, the list with round
 * check marks and the button at the bottom. Picking somebody clears the search.
 */
export function MemberPicker({
  excludeToken,
  selected,
  onChange,
  action,
  actionDisabled,
  busy,
  onAction,
}: {
  excludeToken: string | null
  selected: UserView[]
  onChange: (users: UserView[]) => void
  action: string
  actionDisabled?: boolean
  busy?: boolean
  onAction: () => void
}) {
  const [query, setQuery] = useState('')
  const search = useUserSearch(query, excludeToken)
  const field = useRef<HTMLInputElement>(null)
  const chosen = new Set(selected.map((u) => u.id))

  function toggle(u: UserView) {
    onChange(chosen.has(u.id) ? selected.filter((s) => s.id !== u.id) : [...selected, u])
    setQuery('')
    field.current?.focus()
  }

  return (
    <>
      <div className="search">
        <input
          ref={field}
          className="input search-input"
          placeholder="Name suchen"
          value={query}
          autoFocus
          disabled={busy}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && search.users?.[0]) toggle(search.users[0])
            if (e.key === 'Backspace' && query === '' && selected.length > 0) onChange(selected.slice(0, -1))
          }}
        />
      </div>
      <Chips
        items={selected}
        onRemove={(id) => {
          const u = selected.find((s) => s.id === id)
          if (u) toggle(u)
        }}
      />
      <div className="people-list">
        <UserResults
          {...search}
          row={(u) => (
            <PersonRow
              key={u.id}
              name={u.name}
              userId={u.id}
              subline={u.subline}
              label={`${u.name}, ${chosen.has(u.id) ? 'ausgewählt' : 'nicht ausgewählt'}`}
              onClick={() => toggle(u)}
              disabled={busy}
              trailing={<Check on={chosen.has(u.id)} />}
            />
          )}
        />
      </div>
      <footer className="pane-footer">
        <button className="button-primary" onClick={onAction} disabled={actionDisabled || busy}>
          {busy ? <span className="spinner tiny light" aria-label="Bitte warten" /> : action}
        </button>
      </footer>
    </>
  )
}
