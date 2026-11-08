import { useEffect, useRef } from 'react'
import { closeRoom, useStore } from './store'
import { Login } from './components/Login'
import { cycleRoom, Sidebar } from './components/Sidebar'
import { Chat } from './components/Chat'

export function App() {
  const session = useStore((s) => s.session)
  const activeToken = useStore((s) => s.activeToken)
  const room = useStore((s) => s.rooms.find((r) => r.token === s.activeToken) ?? null)
  const search = useRef<HTMLInputElement>(null)
  const composer = useRef<HTMLTextAreaElement>(null)

  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      const mod = e.ctrlKey || e.metaKey
      if (mod && (e.key === 'k' || e.key === 'f')) {
        e.preventDefault()
        search.current?.focus()
        search.current?.select()
      } else if (e.altKey && (e.key === 'ArrowUp' || e.key === 'ArrowDown')) {
        e.preventDefault()
        cycleRoom(e.key === 'ArrowUp' ? -1 : 1)
      } else if (e.key === 'Escape' && document.activeElement === document.body) {
        closeRoom()
      } else if (
        // Typing anywhere goes into the message field (like Signal).
        !mod &&
        !e.altKey &&
        e.key.length === 1 &&
        composer.current &&
        !(document.activeElement instanceof HTMLInputElement) &&
        !(document.activeElement instanceof HTMLTextAreaElement)
      ) {
        composer.current.focus()
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])

  if (session === 'loading') return <div className="splash" />
  if (session === 'loggedOut') return <Login />

  return (
    <div className={`app${activeToken ? ' has-chat' : ''}`}>
      <Sidebar ref={search} />
      {room ? (
        <Chat key={room.token} room={room} ref={composer} />
      ) : (
        <section className="chat empty">
          <p className="muted">Wähle einen Chat aus.</p>
        </section>
      )}
    </div>
  )
}
