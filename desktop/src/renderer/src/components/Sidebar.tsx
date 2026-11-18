import { memo, useCallback, useEffect, useMemo, useRef, useState, type KeyboardEvent, type MouseEvent } from 'react'
import { withAttachment, type CategoryView, type RoomView, type SearchHit, type SectionView } from '@shared/core'
import {
  clearSearch,
  getState,
  moreHits,
  moveHit,
  openCategories,
  openCompose,
  openGroupInfo,
  openHit,
  openRoom,
  openSettings,
  searchable,
  searchKey,
  setQuery,
  setScope,
  shownConnection,
  shownRooms,
  toggleSection,
  useStore,
} from '../store'
import { listTime } from '../format'
import { toast } from '../call/store'
import { Avatar } from './Avatar'
import { RecordingBanners } from '../call/RecordingBanners'
import { NewMessage } from './NewMessage'
import { OwnAvatar } from './AccountMenu'
import { MessageMenu, type MenuItem } from './MessageMenu'
import { NameDialog } from './Categories'
import { isGroupRoom, LEVELS } from './GroupInfo'


/** So many hit rows are drawn at first and added when the end of the list is reached. */
const ROWS_STEP = 100

/** „17 Treffer“ above the hits of one chat (docs/search.md). */
function hitCount(total: number): string {
  return total === 0 ? 'Keine Treffer' : total === 1 ? '1 Treffer' : `${total.toLocaleString('de-DE')} Treffer`
}

export function Sidebar() {
  const searchRef = useRef<HTMLInputElement>(null)
  const sections = useStore((s) => s.sections)
  const query = useStore((s) => s.query)
  const scope = useStore((s) => s.scope)
  const results = useStore((s) => s.hits)
  // The hits belong to what the field says (not to the search text before).
  const current = useStore((s) => s.hitsFor === searchKey(s))
  const chosen = useStore((s) => s.hit)
  const searchFocus = useStore((s) => s.searchFocus)
  const rooms = useStore((s) => s.rooms)
  const activeToken = useStore((s) => s.activeToken)
  const connection = useStore((s) => s.connection)
  const shown = useStore(shownConnection)
  const noRooms = useStore((s) => s.rooms.length === 0)
  const account = useStore((s) => s.account)
  const composing = useStore((s) => s.composing)
  const [menu, setMenu] = useState(false)
  const [canManage, setCanManage] = useState(false)
  const [chatMenu, setChatMenu] = useState<{ token: string; x: number; y: number } | null>(null)
  const now = useNow(30_000)

  // Known after the first room list: ask again when a menu opens.
  useEffect(() => {
    let alive = true
    window.talk
      .categoriesSupported()
      .then((ok) => alive && setCanManage(ok))
      .catch(() => {})
    return () => {
      alive = false
    }
  }, [account, menu, chatMenu])

  const onRoomMenu = useCallback((token: string, x: number, y: number) => setChatMenu({ token, x, y }), [])

  // Ctrl+F, Ctrl+Shift+F and the magnifier of the chat (docs/search.md).
  useEffect(() => {
    if (searchFocus === 0) return
    searchRef.current?.focus()
    searchRef.current?.select()
  }, [searchFocus])

  const avatarVersions = useMemo(() => new Map(rooms.map((r) => [r.token, r.avatarVersion])), [rooms])

  // In one chat the core hands over up to 500 hits: the rows follow in steps, so the list
  // keeps up with the typing.
  const [rowLimit, setRowLimit] = useState(ROWS_STEP)
  const listKey = useStore(searchKey)
  const [limitFor, setLimitFor] = useState(listKey)
  if (limitFor !== listKey) {
    setLimitFor(listKey)
    setRowLimit(ROWS_STEP)
  }
  const chosenAt = chosen && results ? results.hits.findIndex((h) => h.token === chosen.token && h.messageId === chosen.messageId) : -1
  // The arrow keys reach every hit, also one that is not drawn yet.
  if (chosenAt >= rowLimit) setRowLimit(chosenAt + ROWS_STEP)

  if (composing) {
    return (
      <aside className="sidebar">
        <NewMessage />
      </aside>
    )
  }

  const searching = searchable(query)
  const hits = searching ? (results?.hits ?? []) : []
  // Over all chats the list grows by 50 hits anyway (`moreHits`).
  const rows = scope && hits.length > rowLimit ? hits.slice(0, rowLimit) : hits
  const empty = sections.every((s) => s.total === 0)
  const noHits = !searching || (current && hits.length === 0)
  const stillLoading = searching && !scope && !!results && results.chatsDone < results.chatsTotal

  function onSearchKey(e: KeyboardEvent<HTMLInputElement>) {
    if (e.key === 'Escape') clearSearch()
    else if (e.key === 'Backspace' && scope && e.currentTarget.value === '') setScope(null)
    else if ((e.key === 'ArrowDown' || e.key === 'ArrowUp') && !e.altKey && hits.length > 0) {
      e.preventDefault()
      moveHit(e.key === 'ArrowDown' ? 1 : -1)
    } else if (e.key === 'Enter') {
      // Nothing of the key reaches the message field of a chat that opens.
      e.preventDefault()
      const first = scope ? undefined : sections[0]?.rooms[0]
      if (chosen) openHit(chosen)
      else if (first) {
        void openRoom(first.token)
        setQuery('')
      } else if (hits[0]) openHit(hits[0])
    }
  }
  // Nothing to list yet (docs/connection.md „Leere Chatliste“): the circle in the middle
  // stands for the one in the header. `noRooms` is ahead of the sections by one answer.
  const loadingList = empty && noRooms && !query && shown === 'connecting'

  return (
    <aside className="sidebar">
      <header className="sidebar-header">
        <OwnAvatar />
        <div className={`sidebar-title${shown === 'offline' ? ' two-line' : ''}`}>
          <h2>
            Chats
            {shown === 'connecting' && !loadingList && <ConnectingSpinner />}
          </h2>
          {shown === 'offline' && (
            <span className="connection-line">
              <ConnectingSpinner />
              Keine Verbindung
            </span>
          )}
        </div>
        <button
          className="icon-button"
          aria-label="Neue Nachricht"
          title={`Neue Nachricht (${navigator.platform.startsWith('Mac') ? '⌘' : 'Strg+'}N)`}
          onClick={openCompose}
        >
          <svg viewBox="0 0 24 24" width="20" height="20" aria-hidden fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinejoin="round" strokeLinecap="round">
            <path d="M12 4.5H6A1.5 1.5 0 0 0 4.5 6v12A1.5 1.5 0 0 0 6 19.5h12a1.5 1.5 0 0 0 1.5-1.5v-6" />
            <path d="M17.5 3.8l2.7 2.7-8.4 8.4-3.3.6.6-3.3z" />
          </svg>
        </button>
        <div className="menu-anchor">
          <button className="icon-button" aria-label="Menü" title="Menü" onClick={() => setMenu((m) => !m)}>
            <svg viewBox="0 0 24 24" width="20" height="20">
              <circle cx="5" cy="12" r="2" fill="currentColor" />
              <circle cx="12" cy="12" r="2" fill="currentColor" />
              <circle cx="19" cy="12" r="2" fill="currentColor" />
            </svg>
          </button>
          {menu && (
            <div className="menu" onMouseLeave={() => setMenu(false)}>
              <button
                className="menu-item"
                onClick={() => {
                  setMenu(false)
                  openSettings()
                }}
              >
                Einstellungen
              </button>
              {canManage && (
                <button
                  className="menu-item"
                  onClick={() => {
                    setMenu(false)
                    openCategories()
                  }}
                >
                  Kategorien verwalten
                </button>
              )}
            </div>
          )}
        </div>
      </header>
      <div className="search">
        <div className="search-box">
          {scope && (
            <span className="search-scope">
              <span className="ellipsis">in {rooms.find((r) => r.token === scope.token)?.name ?? scope.name}</span>
              <button
                className="search-scope-clear"
                aria-label="In allen Chats suchen"
                title="In allen Chats suchen"
                onClick={() => {
                  setScope(null)
                  searchRef.current?.focus()
                }}
              >
                <svg viewBox="0 0 12 12" width="10" height="10" aria-hidden>
                  <path d="M2.5 2.5l7 7M9.5 2.5l-7 7" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" />
                </svg>
              </button>
            </span>
          )}
          <input
            ref={searchRef}
            className="search-field"
            placeholder="Suchen"
            aria-label={scope ? `In ${scope.name} suchen` : 'Suchen'}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={onSearchKey}
          />
        </div>
      </div>
      <RecordingBanners />
      <nav
        className="room-list"
        aria-label={scope ? 'Treffer' : 'Chats'}
        onScroll={(e) => {
          const el = e.currentTarget
          if (el.scrollHeight - el.scrollTop - el.clientHeight > 300) return
          if (rows.length < hits.length) setRowLimit(rowLimit + ROWS_STEP)
          else moreHits()
        }}
      >
        {/* With the mark „in {Chat}“ the list shows only the hits of that chat. */}
        {!scope &&
          sections.map((section) => (
            <div key={section.id} role="group" aria-label={section.title || undefined}>
              {section.title &&
                (section.kind === 'archived' ? (
                  <h3 className="list-section">{section.title}</h3>
                ) : (
                  <SectionHeader section={section} />
                ))}
              {section.rooms.map((room) => (
                <RoomItem
                  key={`${section.id}:${room.token}`}
                  room={room}
                  active={room.token === activeToken}
                  now={now}
                  onMenu={onRoomMenu}
                />
              ))}
            </div>
          ))}
        {scope && searching && current && results && (
          <p className="list-section hit-count" role="status">
            {hitCount(results.total)}
          </p>
        )}
        {hits.length > 0 && (
          <div role="group" aria-label={scope ? undefined : 'Nachrichten'}>
            {!scope && <h3 className="list-section">Nachrichten</h3>}
            {rows.map((hit) => (
              <HitRow
                key={`${hit.token}:${hit.messageId}`}
                hit={hit}
                inChat={!!scope}
                active={chosen?.token === hit.token && chosen.messageId === hit.messageId}
                now={now}
                version={avatarVersions.get(hit.token)}
              />
            ))}
          </div>
        )}
        {stillLoading && (
          <p className="muted hits-loading" role="status">
            Ältere Nachrichten werden noch geladen ({results.chatsDone} von {results.chatsTotal} Chats)
          </p>
        )}
        {!scope && empty && query && noHits && <p className="muted center pad">Keine Treffer</p>}
        {!scope && empty && !query && noRooms && connection === 'online' && <p className="muted center pad">Noch keine Chats</p>}
        {!scope && loadingList && <div className="spinner small quiet list-loading" role="status" aria-label="Verbinde …" />}
      </nav>
      {chatMenu && (
        <ChatMenu
          key={chatMenu.token}
          {...chatMenu}
          categoriesSupported={canManage}
          onClose={() => setChatMenu(null)}
        />
      )}
    </aside>
  )
}

/**
 * A message found by the search (docs/search.md „Trefferzeile“): picture and name of the
 * chat, or with the mark „in {Chat}“ of the sender; below it the excerpt with the search
 * words bold.
 */
const HitRow = memo(function HitRow({
  hit,
  inChat,
  active,
  now,
  version,
}: {
  hit: SearchHit
  inChat: boolean
  active: boolean
  now: number
  version?: string
}) {
  const row = useRef<HTMLButtonElement>(null)
  // The arrow keys of the search field move the choice: it stays in sight.
  useEffect(() => {
    if (active) row.current?.scrollIntoView({ block: 'nearest' })
  }, [active])
  const sender = hit.isOwn ? 'Du' : hit.actorName
  return (
    <button
      ref={row}
      className={`room hit${active ? ' active' : ''}`}
      onClick={() => openHit(hit)}
      aria-current={active}
      data-hit={hit.messageId}
      data-hit-token={hit.token}
    >
      {inChat ? (
        <Avatar name={hit.actorName} userId={hit.actorType === 'users' ? hit.actorId : undefined} />
      ) : (
        <Avatar name={hit.roomName} token={hit.token} version={version} />
      )}
      <div className="room-body">
        <div className="room-top">
          <span className="room-title">
            <span className="room-name">{inChat ? sender : hit.roomName}</span>
          </span>
          <span className="room-time">{listTime(hit.timestamp, now)}</span>
        </div>
        <div className="room-bottom">
          <span className="room-preview">
            {!inChat && hit.showActor && `${sender}: `}
            {hit.snippet.map((part, i) => (part.hit ? <strong key={i}>{part.text}</strong> : part.text))}
          </span>
        </div>
      </div>
    </button>
  )
})

/** Head of a section: the whole row opens and closes it; the chevron turns down when open. */
function SectionHeader({ section }: { section: SectionView }) {
  const hidden = section.collapsed ? section.total - section.rooms.length : 0
  const label = [
    section.kind === 'custom' ? `${section.title}, Kategorie` : section.title,
    hidden > 0 ? (hidden === 1 ? '1 Chat ausgeblendet' : `${hidden} Chats ausgeblendet`) : '',
  ]
    .filter(Boolean)
    .join(', ')
  return (
    <h3 className="list-section section-head">
      <button
        className="section-toggle"
        aria-expanded={!section.collapsed}
        aria-label={label}
        onClick={() => void toggleSection(section.id)}
      >
        <span className="ellipsis">{section.title}</span>
        <svg className={`chevron${section.collapsed ? '' : ' open'}`} viewBox="0 0 16 16" width="12" height="12" aria-hidden>
          <path fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" d="M6 3.5l4.5 4.5L6 12.5" />
        </svg>
      </button>
    </h3>
  )
}

/** docs/connection.md: small, thin and muted, next to a title. */
export function ConnectingSpinner() {
  return <span className="spinner tiny quiet" role="status" aria-label="Verbinde …" />
}

/** Current time, refreshed every `intervalMs` (relative times in the list). */
function useNow(intervalMs: number): number {
  const [now, setNow] = useState(Date.now)
  useEffect(() => {
    const id = setInterval(() => setNow(Date.now()), intervalMs)
    return () => clearInterval(id)
  }, [intervalMs])
  return now
}

const RoomItem = memo(function RoomItem({
  room,
  active,
  now,
  onMenu,
}: {
  room: RoomView
  active: boolean
  now: number
  onMenu: (token: string, x: number, y: number) => void
}) {
  const unread = room.unreadMessages > 0
  const muted = room.notificationLevel === 3
  // „Nur Erwähnungen“ and „Aus“: the number in grey, unless we were mentioned.
  const quiet = muted || (room.notificationLevel === 2 && !room.unreadMention)
  const last = room.lastMessage
  let prefix = ''
  let preview = ''
  if (last) {
    preview = withAttachment(last.text, last.attachment, last.location)
    if (!preview) preview = 'Nachricht gelöscht'
    else if (!last.isSystem && room.roomType !== 1 && room.roomType !== 6)
      prefix = `${last.isOwn ? 'Du' : last.author.split(' ')[0]}: `
  }
  return (
    <button
      className={`room${active ? ' active' : ''}${unread ? ' unread' : ''}`}
      onClick={() => void openRoom(room.token)}
      aria-current={active}
      aria-haspopup="menu"
      data-token={room.token}
      onContextMenu={(e: MouseEvent<HTMLButtonElement>) => {
        e.preventDefault()
        // The context-menu key also ends up here (without a pointer position).
        if (e.clientX === 0 && e.clientY === 0) {
          const r = e.currentTarget.getBoundingClientRect()
          onMenu(room.token, r.left + 24, r.top + r.height / 2)
        } else onMenu(room.token, e.clientX, e.clientY)
      }}
      onKeyDown={(e: KeyboardEvent<HTMLButtonElement>) => {
        if (e.key === 'ContextMenu' || (e.shiftKey && e.key === 'F10')) {
          e.preventDefault()
          const r = e.currentTarget.getBoundingClientRect()
          onMenu(room.token, r.left + 24, r.top + r.height / 2)
        }
      }}
    >
      <Avatar name={room.name} token={room.token} version={room.avatarVersion} />
      <div className="room-body">
        <div className="room-top">
          <span className="room-title">
            <span className="room-name">{room.name}</span>
            {muted && <BellOffIcon />}
          </span>
          <span className="room-time">{listTime(room.lastActivity, now)}</span>
        </div>
        <div className="room-bottom">
          <span className={`room-preview${last?.isSystem ? ' system' : ''}`}>
            {prefix && <span className="room-prefix">{prefix}</span>}
            {preview}
          </span>
          {(room.hasCall || unread) && (
            <span className="room-indicators">
              {room.hasCall && <span className="call-dot" title="Laufender Anruf" />}
              {unread && room.unreadMention && (
                <span className="badge mention" title="Erwähnt">
                  @
                </span>
              )}
              {unread && (
                <span className={`badge${quiet ? ' quiet' : ''}`}>{room.unreadMessages > 99 ? '99+' : room.unreadMessages}</span>
              )}
            </span>
          )}
        </div>
      </div>
    </button>
  )
})

/** Alt+↑/↓ cycles through the conversations shown in the list. */
export function cycleRoom(delta: number): void {
  const { activeToken } = getState()
  const list = shownRooms()
  if (list.length === 0) return
  const i = list.findIndex((r) => r.token === activeToken)
  const next = list[(i + delta + list.length) % list.length] ?? list[0]
  void openRoom(next.token)
}

/** Small struck-through bell behind the name of a chat set to „Aus“. */
function BellOffIcon() {
  return (
    <svg className="muted-bell" viewBox="0 0 16 16" width="14" height="14" role="img" aria-label="stummgeschaltet">
      <path
        fill="none"
        stroke="currentColor"
        strokeWidth="1.4"
        strokeLinecap="round"
        strokeLinejoin="round"
        d="M4.5 10.5V7.5a3.5 3.5 0 0 1 6.2-2.2M11.5 7.5v3l1 1.5h-8M6.8 13.5a1.4 1.4 0 0 0 2.4 0M2.5 2.5l11 11"
      />
    </svg>
  )
}

/** Own categories of the menu „Kategorie ›“ (docs/settings.md). */
function useCustomCategories(enabled: boolean): CategoryView[] {
  const rooms = useStore((s) => s.rooms)
  const [list, setList] = useState<CategoryView[]>([])
  useEffect(() => {
    if (!enabled) return
    let alive = true
    window.talk
      .categories()
      .then((l) => alive && setList(l.filter((c) => c.kind === 'custom')))
      .catch(() => {})
    return () => {
      alive = false
    }
  }, [enabled, rooms])
  return list
}

/**
 * Chat menu (docs/settings.md): right click, context-menu key or Shift+F10 on a chat in the
 * list. Changes come back as `roomsChanged`, so the entries show the new state.
 */
function ChatMenu({
  token,
  x,
  y,
  categoriesSupported,
  onClose,
}: {
  token: string
  x: number
  y: number
  categoriesSupported: boolean
  onClose: () => void
}) {
  const room = useStore((s) => s.rooms.find((r) => r.token === token) ?? null)
  const categories = useCustomCategories(categoriesSupported)
  const [menuOpen, setMenuOpen] = useState(true)
  const [naming, setNaming] = useState(false)
  const [busy, setBusy] = useState(false)
  const closeMenu = useCallback(() => setMenuOpen(false), [])
  const trigger = useRef(document.activeElement as HTMLElement | null)

  // The focus goes back to the chat row (keyboard).
  const close = useCallback(() => {
    onClose()
    const t = trigger.current
    if (t?.isConnected && (document.activeElement === document.body || document.activeElement === null)) t.focus()
  }, [onClose])

  useEffect(() => {
    if (!room || (!menuOpen && !naming)) close()
  }, [room, menuOpen, naming, close])

  if (!room) return null

  const report = (p: Promise<string | null>) => void p.catch(() => 'Keine Verbindung').then((e) => e && toast(e))
  const oneToOne = room.roomType === 1 || room.roomType === 5 || room.roomType === 6
  const items: MenuItem[] = []
  if (room.unreadMessages > 0)
    items.push({ label: 'Als gelesen markieren', onSelect: () => report(window.talk.markRoomRead(token)) })
  items.push({
    label: room.isFavorite ? 'Aus Favoriten entfernen' : 'Zu Favoriten hinzufügen',
    onSelect: () => report(window.talk.setFavorite(token, !room.isFavorite)),
  })
  if (categoriesSupported) {
    const own = categories.map((c) => c.id)
    items.push({
      kind: 'submenu',
      label: 'Kategorie',
      items: [
        ...categories.map(
          (c): MenuItem => ({
            kind: 'check',
            label: c.name,
            checked: room.tagIds.includes(c.id),
            keepOpen: true,
            onSelect: () => {
              // The newest state: the menu stays open while the list follows.
              const now = getState().rooms.find((r) => r.token === token)?.tagIds ?? room.tagIds
              const ids = now.filter((id) => own.includes(id))
              report(window.talk.setRoomCategories(token, ids.includes(c.id) ? ids.filter((id) => id !== c.id) : [...ids, c.id]))
            },
          }),
        ),
        ...(categories.length > 0 ? [{ kind: 'separator' } as MenuItem] : []),
        { label: 'Neue Kategorie …', onSelect: () => setNaming(true) },
      ],
    })
  }
  items.push({
    kind: 'submenu',
    label: 'Benachrichtigungen',
    items: LEVELS.filter((l) => !(oneToOne && l.level === 2)).map(
      (l): MenuItem => ({
        kind: 'radio',
        label: l.label,
        checked: room.notificationLevel === l.level,
        onSelect: () => {
          if (room.notificationLevel !== l.level) report(window.talk.setNotificationLevel(token, l.level))
        },
      }),
    ),
  })
  if (isGroupRoom(room))
    items.push({
      label: 'Gruppeneinstellungen',
      onSelect: () => {
        void openRoom(token)
        openGroupInfo(token)
      },
    })

  async function create(name: string) {
    setBusy(true)
    const res = await window.talk.createCategory(name, [token]).catch(() => ({ id: null, error: 'Keine Verbindung' }))
    setBusy(false)
    if (res.error || !res.id) toast(res.error ?? 'Kategorie konnte nicht erstellt werden')
    else setNaming(false)
  }

  return (
    <>
      {menuOpen && <MessageMenu x={x} y={y} items={items} label={`Chat-Menü ${room.name}`} onClose={closeMenu} />}
      {naming && (
        <NameDialog
          title="Neue Kategorie"
          initial=""
          confirm="Erstellen"
          busy={busy}
          onCancel={() => setNaming(false)}
          onConfirm={(name) => void create(name)}
        />
      )}
    </>
  )
}
