import { forwardRef, memo, useEffect, useState } from 'react'
import { withAttachment, type RoomView, type SectionView } from '@shared/core'
import {
  getState,
  openCategories,
  openCompose,
  openRoom,
  setQuery,
  shownRooms,
  toggleSection,
  useStore,
} from '../store'
import { listTime } from '../format'
import { Avatar } from './Avatar'
import { RecordingBanners } from '../call/RecordingBanners'
import { NewMessage } from './NewMessage'

export const Sidebar = forwardRef<HTMLInputElement>(function Sidebar(_props, searchRef) {
  const sections = useStore((s) => s.sections)
  const query = useStore((s) => s.query)
  const activeToken = useStore((s) => s.activeToken)
  const online = useStore((s) => s.online)
  const account = useStore((s) => s.account)
  const composing = useStore((s) => s.composing)
  const [menu, setMenu] = useState(false)
  const [canManage, setCanManage] = useState(false)
  const now = useNow(30_000)

  // Known after the first room list: ask again when the menu opens.
  useEffect(() => {
    let alive = true
    window.talk
      .categoriesSupported()
      .then((ok) => alive && setCanManage(ok))
      .catch(() => {})
    return () => {
      alive = false
    }
  }, [account, menu])

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
        <button className="own-avatar" aria-label="Konto" title={account?.user} onClick={() => setMenu((m) => !m)}>
          <Avatar name={account?.user ?? '?'} size={28} />
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
              <div className="menu-info">
                {account?.user}
                <span className="muted">{account?.server.replace(/^https?:\/\//, '')}</span>
              </div>
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
              <button className="menu-item danger" onClick={() => void window.talk.logout()}>
                Abmelden
              </button>
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
              <RoomItem key={`${section.id}:${room.token}`} room={room} active={room.token === activeToken} now={now} />
            ))}
          </div>
        ))}
        {empty && <p className="muted center pad">{query ? 'Keine Treffer' : 'Noch keine Chats'}</p>}
      </nav>
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

const RoomItem = memo(function RoomItem({ room, active, now }: { room: RoomView; active: boolean; now: number }) {
  const unread = room.unreadMessages > 0
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
    >
      <Avatar name={room.name} token={room.token} version={room.avatarVersion} />
      <div className="room-body">
        <div className="room-top">
          <span className="room-name">{room.name}</span>
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
              {unread && <span className="badge">{room.unreadMessages > 99 ? '99+' : room.unreadMessages}</span>}
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
