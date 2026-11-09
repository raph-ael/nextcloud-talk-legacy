import { useEffect, useRef, useState, type CSSProperties, type PointerEvent } from 'react'
import { closeRoom, useStore } from './store'
import { Login } from './components/Login'
import { cycleRoom, Sidebar } from './components/Sidebar'
import { Chat } from './components/Chat'
import { CallLayer, ReturnToCallBar } from './call/CallScreen'

/** Conversation list width like Signal Desktop: 320 px by default, at least 280 px. */
const SIDEBAR_DEFAULT = 320
const SIDEBAR_MIN = 280
const WIDTH_KEY = 'sidebarWidth'

function clampWidth(w: number): number {
  return Math.round(Math.max(SIDEBAR_MIN, Math.min(w, Math.max(SIDEBAR_MIN, window.innerWidth * 0.5))))
}

function storedWidth(): number {
  try {
    const v = Number(localStorage.getItem(WIDTH_KEY))
    return v > 0 ? v : SIDEBAR_DEFAULT
  } catch {
    return SIDEBAR_DEFAULT
  }
}

function useSidebarWidth() {
  const [width, setWidth] = useState(storedWidth)
  const current = useRef(width)
  current.current = width
  const drag = useRef<{ x: number; w: number } | null>(null)
  const [, relayout] = useState(0)
  useEffect(() => {
    // The maximum depends on the window width.
    const onResize = () => relayout((n) => n + 1)
    window.addEventListener('resize', onResize)
    return () => window.removeEventListener('resize', onResize)
  }, [])
  const handle = {
    onPointerDown(e: PointerEvent<HTMLDivElement>) {
      e.currentTarget.setPointerCapture(e.pointerId)
      drag.current = { x: e.clientX, w: clampWidth(width) }
      document.body.classList.add('resizing')
    },
    onPointerMove(e: PointerEvent<HTMLDivElement>) {
      if (!drag.current) return
      current.current = clampWidth(drag.current.w + e.clientX - drag.current.x)
      setWidth(current.current)
    },
    onPointerUp() {
      drag.current = null
      document.body.classList.remove('resizing')
      try {
        localStorage.setItem(WIDTH_KEY, String(current.current))
      } catch {
        // Not persisted; the default applies next time.
      }
    },
    onDoubleClick() {
      setWidth(SIDEBAR_DEFAULT)
      try {
        localStorage.removeItem(WIDTH_KEY)
      } catch {
        // ignore
      }
    },
  }
  return { width: clampWidth(width), handle }
}

export function App() {
  const session = useStore((s) => s.session)
  const activeToken = useStore((s) => s.activeToken)
  const room = useStore((s) => s.rooms.find((r) => r.token === s.activeToken) ?? null)
  const search = useRef<HTMLInputElement>(null)
  const composer = useRef<HTMLTextAreaElement>(null)
  const { width, handle } = useSidebarWidth()

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
    <div className="app-shell">
    <ReturnToCallBar />
    <div className={`app${activeToken ? ' has-chat' : ''}`} style={{ '--sidebar-w': `${width}px` } as CSSProperties}>
      <Sidebar ref={search} />
      <div
        className="sidebar-resize"
        role="separator"
        aria-orientation="vertical"
        aria-label="Breite der Chatliste"
        title="Ziehen zum Ändern der Breite, Doppelklick setzt zurück"
        {...handle}
      />
      {room ? (
        <Chat key={room.token} room={room} ref={composer} />
      ) : (
        <section className="chat empty">
          <p className="muted">Wähle einen Chat aus.</p>
        </section>
      )}
    </div>
    <CallLayer />
    </div>
  )
}
