import { forwardRef, memo, useCallback, useEffect, useRef, useState, type KeyboardEvent, type MouseEvent } from 'react'
import { withAttachment, type CategoryView, type RoomView, type SectionView } from '@shared/core'
import {
  getState,
  openCategories,
  openCompose,
  openGroupInfo,
  openRoom,
  openSettings,
  setQuery,
  shownRooms,
  toggleSection,
  useStore,
} from '../store'
import { listTime } from '../format'
import { toast } from '../call/store'
import { Avatar } from './Avatar'
import { RecordingBanners } from '../call/RecordingBanners'
import { NewMessage } from './NewMessage'
import { MessageMenu, type MenuItem } from './MessageMenu'
import { NameDialog } from './Categories'
import { isGroupRoom, LEVELS } from './GroupInfo'

const MOD = navigator.platform.startsWith('Mac') ? '⌘' : 'Strg+'

export const Sidebar = forwardRef<HTMLInputElement>(function Sidebar(_props, searchRef) {
  const sections = useStore((s) => s.sections)
  const query = useStore((s) => s.query)
  const activeToken = useStore((s) => s.activeToken)
  const online = useStore((s) => s.online)
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

  if (composing) {
    return (
      <aside className="sidebar">
        <NewMessage />
      </aside>
    )
  }

  const empty = sections.every((s) => s.total === 0)

  return (
    <aside className="sidebar">
      <header className="sidebar-header">
        <button className="own-avatar" aria-label="Einstellungen" title={`Einstellungen (${MOD},)`} onClick={() => openSettings()}>
          <Avatar name={account?.user ?? '?'} userId={account?.userId} size={28} />
        </button>
        <h2>Chats</h2>
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
        <input
          ref={searchRef}
          className="input search-input"
          placeholder="Suchen"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Escape') setQuery('')
            const first = sections[0]?.rooms[0]
            if (e.key === 'Enter' && first) {
              void openRoom(first.token)
              setQuery('')
            }
          }}
        />
      </div>
      {!online && <OfflineBanner />}
      <RecordingBanners />
      <nav className="room-list" aria-label="Chats">
        {sections.map((section) => (
          <div key={section.id} role="group" aria-label={section.title || undefined}>
            {section.title && <SectionHeader section={section} />}
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
        {empty && <p className="muted center pad">{query ? 'Keine Treffer' : 'Noch keine Chats'}</p>}
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

export function OfflineBanner() {
  return <div className="offline">Keine Verbindung – wird erneut versucht …</div>
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
    preview = withAttachment(last.text, last.attachment)
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
