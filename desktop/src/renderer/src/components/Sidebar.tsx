import { forwardRef, memo, useEffect, useMemo, useState } from 'react'
import { withAttachment, type RoomView } from '@shared/core'
import { getState, openRoom, useStore } from '../store'
import { listTime } from '../format'
import { Avatar } from './Avatar'

export const Sidebar = forwardRef<HTMLInputElement>(function Sidebar(_props, searchRef) {
  const rooms = useStore((s) => s.rooms)
  const activeToken = useStore((s) => s.activeToken)
  const online = useStore((s) => s.online)
  const account = useStore((s) => s.account)
  const [query, setQuery] = useState('')
  const [menu, setMenu] = useState(false)
  const now = useNow(30_000)

  const visible = useMemo(() => {
    const q = query.trim().toLocaleLowerCase('de')
    return rooms.filter((r) => !r.isArchived && (!q || r.name.toLocaleLowerCase('de').includes(q)))
  }, [rooms, query])
  const favorites = visible.filter((r) => r.isFavorite)
  const others = visible.filter((r) => !r.isFavorite)

  const row = (room: RoomView) => <RoomItem key={room.token} room={room} active={room.token === activeToken} now={now} />

  return (
    <aside className="sidebar">
      <header className="sidebar-header">
        <button className="own-avatar" aria-label="Konto" title={account?.user} onClick={() => setMenu((m) => !m)}>
          <Avatar name={account?.user ?? '?'} size={28} />
        </button>
        <h2>Chats</h2>
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
            if (e.key === 'Enter' && visible[0]) {
              void openRoom(visible[0].token)
              setQuery('')
            }
          }}
        />
      </div>
      {!online && <OfflineBanner />}
      <nav className="room-list" aria-label="Chats">
        {favorites.length > 0 && (
          <>
            <h3 className="list-section">Favoriten</h3>
            {favorites.map(row)}
            {others.length > 0 && <h3 className="list-section">Chats</h3>}
          </>
        )}
        {others.map(row)}
        {visible.length === 0 && <p className="muted center pad">{query ? 'Keine Treffer' : 'Noch keine Chats'}</p>}
      </nav>
    </aside>
  )
})

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

/** Alt+↑/↓ cycles through the visible conversations. */
export function cycleRoom(delta: number): void {
  const { rooms, activeToken } = getState()
  const list = rooms.filter((r) => !r.isArchived)
  if (list.length === 0) return
  const i = list.findIndex((r) => r.token === activeToken)
  const next = list[(i + delta + list.length) % list.length] ?? list[0]
  void openRoom(next.token)
}
