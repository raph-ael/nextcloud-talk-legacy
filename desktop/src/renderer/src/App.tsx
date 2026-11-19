import { useEffect, useRef, useState, type CSSProperties, type PointerEvent } from 'react'
import { closeRoom, closeSettings, focusSearch, getState, openCompose, openSettings, useStore } from './store'
import { GroupInfo } from './components/GroupInfo'
import { Settings } from './components/Settings'
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
  const groupInfo = useStore((s) => s.groupInfo)
  const settings = useStore((s) => s.settings)
  const adding = useStore((s) => s.adding)
  // Tokens of different servers overlap: another account means another chat.
  const accountKey = useStore((s) => s.account?.id ?? '')
  const composer = useRef<HTMLTextAreaElement>(null)
  const { width, handle } = useSidebarWidth()

  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      const mod = e.ctrlKey || e.metaKey
      const inSettings = !!getState().settings
      if (mod && !e.shiftKey && !e.altKey && e.key === ',') {
        // „Einstellungen“ (docs/settings.md); on macOS the app menu has ⌘, as well.
        e.preventDefault()
        openSettings()
      } else if (mod && !e.shiftKey && !e.altKey && e.key.toLowerCase() === 'n') {
        e.preventDefault()
        closeSettings()
        openCompose()
      } else if (mod && !e.altKey && (e.key.toLowerCase() === 'f' || e.key === 'k')) {
        // docs/search.md: Ctrl+F searches all chats, Ctrl+Shift+F the open one.
        e.preventDefault()
        closeSettings()
        focusSearch(e.shiftKey && e.key !== 'k')
      } else if (inSettings) {
        // The settings handle their own keys (Esc goes back to the chats).
      } else if (e.altKey && (e.key === 'ArrowUp' || e.key === 'ArrowDown')) {
        e.preventDefault()
        cycleRoom(e.key === 'ArrowUp' ? -1 : 1)
      } else if (e.key === 'Escape' && document.activeElement === document.body) {
        // Steps of „Neue Nachricht“ and the group settings handle Esc themselves.
        const s = getState()
        if (!e.defaultPrevented && !s.composing && !s.groupInfo) closeRoom()
      } else if (
        // Typing anywhere goes into the message field (like Signal).
        !mod &&
        !e.altKey &&
        e.key.length === 1 &&
        composer.current &&
        !getState().groupInfo &&
        !(document.activeElement instanceof HTMLInputElement) &&
        !(document.activeElement instanceof HTMLTextAreaElement) &&
        // Not out of a dialog or menu, and Space presses the focused button.
        !document.querySelector('.modal, .menu-layer, .viewer') &&
        !(e.key === ' ' && document.activeElement instanceof HTMLButtonElement)
      ) {
        composer.current.focus()
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])

  if (session === 'loading') return <div className="splash" />
  if (session === 'loggedOut') return <Login />
  if (adding) {
    // „Konto hinzufügen“: the other accounts go on, a call keeps its view.
    return (
      <div className="app-shell">
        <Login adding />
        <CallLayer />
      </div>
    )
  }

  return (
    <div className="app-shell">
    <ReturnToCallBar />
    <div className="app-stage" style={{ '--sidebar-w': `${width}px` } as CSSProperties}>
    {/* The settings cover chat list and chat; both stay mounted (draft, scroll position). */}
    <div className={`app${activeToken ? ' has-chat' : ''}`} inert={!!settings}>
      <Sidebar />
      <div
        className="sidebar-resize"
        role="separator"
        aria-orientation="vertical"
        aria-label="Breite der Chatliste"
        title="Ziehen zum Ändern der Breite, Doppelklick setzt zurück"
        {...handle}
      />
      {room ? (
        <div className="chat-pane">
          {/* The chat stays mounted under the settings (scroll position, draft). */}
          <div className="chat-slot" inert={groupInfo === room.token}>
            <Chat key={`${accountKey}:${room.token}`} room={room} ref={composer} />
          </div>
          {groupInfo === room.token && <GroupInfo key={`${accountKey}:${room.token}`} room={room} />}
        </div>
      ) : (
        <section className="chat empty">
          <p className="muted">Wähle einen Chat aus.</p>
        </section>
      )}
    </div>
    {settings && <Settings />}
    </div>
    <CallLayer />
    </div>
  )
}
