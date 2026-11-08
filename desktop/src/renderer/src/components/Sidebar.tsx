import { forwardRef, memo, useMemo, useState } from 'react'
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

  const visible = useMemo(() => {
    const q = query.trim().toLocaleLowerCase('de')
    return rooms.filter((r) => !r.isArchived && (!q || r.name.toLocaleLowerCase('de').includes(q)))
  }, [rooms, query])

  return (
    <aside className="sidebar">
      <header className="sidebar-header">
        <h2>Chats</h2>
        <div className="menu-anchor">
          <button className="icon-button" aria-label="Menü" onClick={() => setMenu((m) => !m)}>
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
      {!online && <div className="offline">Keine Verbindung – wird erneut versucht …</div>}
      <nav className="room-list" aria-label="Chats">
        {visible.map((room) => (
          <RoomItem key={room.token} room={room} active={room.token === activeToken} />
        ))}
        {visible.length === 0 && <p className="muted center pad">{query ? 'Keine Treffer' : 'Noch keine Chats'}</p>}
      </nav>
    </aside>
  )
})

const RoomItem = memo(function RoomItem({ room, active }: { room: RoomView; active: boolean }) {
  const unread = room.unreadMessages > 0
  const last = room.lastMessage
  let preview = ''
  if (last) {
    const text = withAttachment(last.text, last.attachment)
    if (last.isSystem || room.roomType === 1 || room.roomType === 6) preview = text
    else preview = `${last.isOwn ? 'Du' : last.author.split(' ')[0]}: ${text}`
    if (!text) preview = 'Nachricht gelöscht'
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
          <span className="room-name">
            {room.isFavorite && (
              <svg className="pin" viewBox="0 0 24 24" width="12" height="12" aria-label="Angeheftet">
                <path fill="currentColor" d="M16 3l5 5-3 1-4 4 1 5-2 2-4-4-5 5-1-1 5-5-4-4 2-2 5 1 4-4z" />
              </svg>
            )}
            {room.name}
          </span>
          <span className="room-time">{listTime(room.lastActivity)}</span>
        </div>
        <div className="room-bottom">
          <span className={`room-preview${last?.isSystem ? ' system' : ''}`}>{preview}</span>
          {room.hasCall && <span className="call-dot" title="Laufender Anruf" />}
          {unread && (
            <span className={`badge${room.unreadMention ? ' mention' : ''}`}>
              {room.unreadMention ? '@' : room.unreadMessages > 99 ? '99+' : room.unreadMessages}
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
