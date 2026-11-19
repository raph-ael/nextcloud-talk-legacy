import { forwardRef, Fragment, memo, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type DragEvent } from 'react'
import { locationName, withAttachment, type RoomView, type SendError } from '@shared/core'
import {
  discard,
  focusSearch,
  loadNewer,
  loadOlder,
  markVisibleRead,
  onBeforeAppend,
  openGroupInfo,
  retry,
  countsAsUnread,
  setChatAtBottom,
  showEnd,
  showMessage,
  shownConnection,
  useStore,
  type UiMessage,
} from '../store'
import { isGroupRoom } from './GroupInfo'
import { colorFor, dayLabel, fullTime, isJumboEmoji, locationLink, messageTime, sameDay, searchWords, splitLinks } from '../format'
import { Markdown, marked, plain, type Marks } from './Markdown'
import { Avatar } from './Avatar'
import { ChatImage, FileRow, isImageAttachment, LocationRow } from './ChatMedia'
import { Composer } from './Composer'
import { VoiceMessage } from './Voice'
import { isAudioAttachment } from '../voice'
import { ImageViewer } from './ImageViewer'
import { ConfirmDialog } from './Modal'
import { MessageMenu, type MenuItem } from './MessageMenu'
import { ConnectingSpinner } from './Sidebar'
import { react, reactable, ReactionBar, ReactionPills, ReactionPopover, WhoReacted } from './Reactions'
import { CallButton } from '../call/CallScreen'
import { toast } from '../call/store'
import { RecordingBanners } from '../call/RecordingBanners'

/** Floating „Zum Ende springen“ (docs/chat.md); always mounted so it can fade in and out. */
function JumpButton({ shown, unread, onJump }: { shown: boolean; unread: number; onJump: () => void }) {
  const label =
    unread > 0
      ? `Zum Ende springen, ${unread} ${unread === 1 ? 'neue Nachricht' : 'neue Nachrichten'}`
      : 'Zum Ende springen'
  return (
    <button
      className={`jump${shown ? ' shown' : ''}`}
      onClick={onJump}
      aria-label={label}
      title={label}
      aria-hidden={!shown}
      tabIndex={shown ? 0 : -1}
    >
      <svg viewBox="0 0 24 24" width="20" height="20" aria-hidden="true">
        <path fill="currentColor" d="M12 16.5l-6-6 1.4-1.4 4.6 4.6 4.6-4.6L18 10.5z" />
      </svg>
      {unread > 0 && <span className="jump-badge">{unread > 99 ? '99+' : unread}</span>}
    </button>
  )
}

/** „N ungelesene Nachrichten“ above the first unread message (docs/chat.md). */
function unreadText(count: number): string {
  return count === 1 ? '1 ungelesene Nachricht' : `${count} ungelesene Nachrichten`
}

/** A window into the history loads on when its end is this close (px). */
const NEWER_AHEAD = 400
/** Where a message stands that the chat jumps to: in the upper third (docs/search.md). */
const JUMP_AT = 0.2

/** The bubble lights up for about 1.5 s and fades (docs/search.md „Aufleuchten“). */
function flash(bubble: HTMLElement): void {
  const color = getComputedStyle(bubble).getPropertyValue('--search-mark').trim() || 'rgb(255 214 10 / 0.45)'
  const on = `inset 0 0 0 999px ${color}`
  for (const a of bubble.getAnimations()) if (a.id === 'flash') a.cancel()
  bubble.animate([{ boxShadow: on }, { boxShadow: on, offset: 0.75 }, { boxShadow: 'inset 0 0 0 999px transparent' }], {
    duration: 2000,
    id: 'flash',
  })
}

/** At most this many pages are loaded to reach the first unread message when opening. */
const MAX_PAGES_TO_LINE = 10
/** The floating date disappears this long after the last scroll event (Desktop). */
const DATE_PILL_MS = 3000

/** The row (message or system message) at the top edge of the visible timeline. */
function topRow(el: HTMLElement): HTMLElement | null {
  const rows = el.querySelectorAll<HTMLElement>('[data-ts]')
  const top = el.getBoundingClientRect().top
  // Rows are in time order, top to bottom: the first one reaching below the top edge.
  let lo = 0
  let hi = rows.length - 1
  let found: HTMLElement | null = null
  while (lo <= hi) {
    const mid = (lo + hi) >> 1
    if (rows[mid].getBoundingClientRect().bottom > top) {
      found = rows[mid]
      hi = mid - 1
    } else lo = mid + 1
  }
  return found
}

function ChatTitle({ room }: { room: RoomView }) {
  // docs/connection.md: the circle next to the name; without a connection the text takes
  // the place of the description.
  const connection = useStore(shownConnection)
  return (
    <>
      <Avatar key={room.avatarVersion} name={room.name} token={room.token} version={room.avatarVersion} size={32} />
      <div className="chat-title">
        <div className="chat-name">
          <h2>{room.name}</h2>
          {connection === 'connecting' && <ConnectingSpinner />}
        </div>
        {connection === 'offline' ? (
          <span className="muted ellipsis">Keine Verbindung</span>
        ) : (
          room.description && <span className="muted ellipsis">{plain(room.description)}</span>
        )}
      </div>
    </>
  )
}

/** Messages of the same author within this window form one visual group. */
const GROUP_WINDOW_S = 3 * 60

// docs/chat.md „Bearbeiten und Löschen“: what goes with a message, computed when the menu opens.
function canEdit(room: RoomView, m: UiMessage): boolean {
  return !m.status && m.editable && room.canChat && (m.editUntil == null || Date.now() / 1000 < m.editUntil)
}
function canDelete(room: RoomView, m: UiMessage): boolean {
  return !m.status && m.deletable && (m.isOwn ? room.canChat : room.canDeleteOthers)
}

function deleteText(m: UiMessage): string {
  const first = m.isOwn
    ? 'Sie wird für alle im Chat gelöscht.'
    : `Die Nachricht von ${m.author} wird für alle im Chat gelöscht.`
  return m.attachment ? `${first} Die Datei selbst bleibt erhalten.` : first
}

/** A message being edited, with the text for the input field (`editSource`). */
export interface Editing {
  msg: UiMessage
  source: string
}

export const Chat = forwardRef<HTMLTextAreaElement, { room: RoomView }>(function Chat({ room }, composerRef) {
  const chat = useStore((s) => s.chats[room.token])
  const [replyTo, setReplyTo] = useState<UiMessage | null>(null)
  const [editing, setEditing] = useState<Editing | null>(null)
  const [deleting, setDeleting] = useState<UiMessage | null>(null)
  const [deleteBusy, setDeleteBusy] = useState(false)
  const [menu, setMenu] = useState<{ msg: UiMessage; x: number; y: number } | null>(null)
  const [viewer, setViewer] = useState<UiMessage | null>(null)
  const [reacting, setReacting] = useState<{
    msg: UiMessage
    anchor: DOMRect
    toggle: HTMLElement | null
    full: boolean
  } | null>(null)
  const [whoReacted, setWhoReacted] = useState<number | null>(null)
  const [dropping, setDropping] = useState(false)
  const addFiles = useRef<((files: File[]) => void) | null>(null)
  const scroller = useRef<HTMLDivElement>(null)
  // Two independent thresholds (docs/chat.md, „Zum Ende springen“): `atBottom` is the tight one
  // for the read marker, `showJump` shows the button only half a screen away from the end.
  const [atBottom, setAtBottom] = useState(true)
  const [showJump, setShowJump] = useState(false)
  // The ref is what the read marker trusts: it is right in the same frame as a scroll we make.
  const atBottomRef = useRef(true)
  const [datePill, setDatePill] = useState<{ label: string; shown: boolean }>({ label: '', shown: false })
  const datePillTimer = useRef(0)
  // Opening at the unread line: done once per opening; until then the read marker waits.
  const placed = useRef(false)
  const pagesLoaded = useRef(0)
  const ownScroll = useRef(false)
  const lastHeight = useRef(0)
  const viewHeight = useRef(0)

  const isGroup = room.roomType !== 1 && room.roomType !== 6
  // A window into the history (docs/search.md): newer messages follow below the list.
  const inWindow = !!chat?.hasNewer
  const windowRef = useRef(inWindow)
  windowRef.current = inWindow
  const loaded = chat?.messages
  // Unsent messages stand at the end of the chat: a window does not show them.
  const messages = useMemo(() => (inWindow ? (loaded ?? []).filter((m) => !m.status) : (loaded ?? [])), [loaded, inWindow])
  const jump = useStore((s) => (s.jump?.token === room.token ? s.jump : null))
  const jumped = useRef(0)
  // The search words of the search in this chat, marked in the bubbles.
  const scopedQuery = useStore((s) => (s.scope?.token === room.token ? s.query : ''))
  const marks = useMemo(() => searchWords(scopedQuery), [scopedQuery])
  // New messages a window keeps for its end: the button counts them.
  const heldCount = useMemo(() => (chat?.held ?? []).filter((m) => !m.isOwn && m.kind !== 'system').length, [chat?.held])
  const unreadLine = useStore((s) => (s.unreadLine?.token === room.token ? s.unreadLine : null))
  // A notification click on this chat while it was open: it is placed once more.
  const reopened = useStore((s) => s.reopened)
  const placedFor = useRef(reopened)

  // The line goes above the first foreign, non-system message after the snapshot, but only
  // once the snapshot itself is loaded (or the whole history): else it would sit too low.
  // Number and place come from what is loaded (robust against a list that was behind and
  // messages arriving twice): at least the room's number when opening.
  const counting = unreadLine ? messages.filter((m) => countsAsUnread(m, unreadLine.from)) : []
  const lineCovered =
    !!unreadLine && (!chat?.hasMore || messages.some((m) => !m.status && m.id <= unreadLine.from))
  const lineAt = lineCovered ? counting[0]?.id : undefined
  const unreadCount = Math.max(unreadLine?.base ?? 0, counting.length)

  useEffect(() => setReplyTo(null), [room.token])

  // Editing ends when the message was deleted meanwhile (leaving the chat unmounts it).
  const editedKind = editing ? messages.find((m) => m.id === editing.msg.id)?.kind : undefined
  useEffect(() => {
    if (editedKind === 'deleted') setEditing(null)
  }, [editedKind])

  const startEdit = useCallback(
    async (m: UiMessage) => {
      const source = await window.talk.editSource(room.token, m.id).catch(() => null)
      setEditing({ msg: m, source: source ?? m.text })
    },
    [room.token],
  )
  const endEdit = useCallback(() => setEditing(null), [])
  // A reply started while editing ends the editing: only one of the two bars shows.
  const reply = useCallback((m: UiMessage) => {
    setEditing(null)
    setReplyTo(m)
  }, [])

  /** Arrow up in the empty field: the last own message that can still be edited. */
  function editLast(): boolean {
    for (let i = messages.length - 1; i >= 0; i--) {
      const m = messages[i]
      if (m.isOwn && canEdit(room, m)) {
        void startEdit(m)
        return true
      }
    }
    return false
  }

  async function confirmDelete(m: UiMessage) {
    setDeleteBusy(true)
    const error = await window.talk.deleteMessage(room.token, m.id).catch(() => 'Nachricht nicht gelöscht')
    setDeleteBusy(false)
    setDeleting(null)
    if (error) toast(error)
  }

  function updateAtBottom(value: boolean) {
    atBottomRef.current = value
    setChatAtBottom(value)
    setAtBottom(value)
  }
  useEffect(() => () => setChatAtBottom(true), [])
  /** The tight threshold of the read marker; the end of a window is not the end of the chat. */
  function atEnd(el: HTMLElement): boolean {
    return !windowRef.current && el.scrollTop > -40
  }
  // A window opens or reaches the end of the chat.
  useLayoutEffect(() => {
    const el = scroller.current
    if (el) updateAtBottom(atEnd(el))
  }, [inWindow])

  // Jump to a message or to the end (docs/search.md „Chat an einer Nachricht öffnen“).
  useLayoutEffect(() => {
    const el = scroller.current
    if (!el || !jump || jumped.current === jump.seq) return
    const hidePill = () => {
      // The floating date belongs to where the chat stood before.
      window.clearTimeout(datePillTimer.current)
      setDatePill((p) => (p.shown ? { ...p, shown: false } : p))
    }
    if (jump.id == null) {
      hidePill()
      jumped.current = jump.seq
      placed.current = true
      if (el.scrollTop !== 0) {
        ownScroll.current = true
        el.scrollTop = 0
      }
      updateAtBottom(true)
      setShowJump(false)
      markVisibleRead(room.token)
      return
    }
    const bubble = el.querySelector<HTMLElement>(`.bubble[data-id="${jump.id}"]`)
    // Not there yet: the list follows.
    if (!bubble) return
    hidePill()
    jumped.current = jump.seq
    placed.current = true
    // Never further than the end (scrollTop 0); the browser stops at the beginning.
    const target = Math.min(
      0,
      el.scrollTop + bubble.getBoundingClientRect().top - el.getBoundingClientRect().top - el.clientHeight * JUMP_AT,
    )
    if (Math.abs(target - el.scrollTop) >= 1) {
      ownScroll.current = true
      el.scrollTop = target
    }
    updateAtBottom(atEnd(el))
    setShowJump(el.scrollTop < -el.clientHeight / 2)
    flash(bubble)
    if (windowRef.current && el.scrollTop > -NEWER_AHEAD) void loadNewer(room.token)
  }, [jump, chat, room.token])

  // A window grows downwards: what is in sight stays where it is (the list is column-reverse,
  // so the browser would keep the distance to the end instead).
  const anchor = useRef<{ row: HTMLElement; top: number } | null>(null)
  useEffect(
    () =>
      onBeforeAppend((token) => {
        const el = scroller.current
        if (!el || token !== room.token) return
        const row = topRow(el) ?? el.querySelector<HTMLElement>('[data-ts]')
        anchor.current = row ? { row, top: row.getBoundingClientRect().top } : null
      }),
    [room.token],
  )
  useLayoutEffect(() => {
    const el = scroller.current
    const kept = anchor.current
    anchor.current = null
    if (!el || !kept || !kept.row.isConnected) return
    const moved = kept.row.getBoundingClientRect().top - kept.top
    if (Math.abs(moved) >= 1) {
      ownScroll.current = true
      el.scrollTop += moved
    }
    if (windowRef.current && el.scrollTop > -NEWER_AHEAD) void loadNewer(room.token)
  }, [chat, room.token])

  // Read marker: whenever new messages are visible in the focused window.
  useEffect(() => {
    if (placed.current && atBottomRef.current) markVisibleRead(room.token)
  }, [room.token, messages.length, atBottom, room.unreadMessages])
  useEffect(() => {
    const onFocus = () => placed.current && atBottomRef.current && markVisibleRead(room.token)
    window.addEventListener('focus', onFocus)
    return () => window.removeEventListener('focus', onFocus)
  }, [room.token])

  // Open at the unread line (docs/chat.md „Öffnen an der Linie“): wait for the server page,
  // load older pages until the line is there, then put it in the upper quarter.
  useLayoutEffect(() => {
    const el = scroller.current
    if (placedFor.current !== reopened) {
      placedFor.current = reopened
      placed.current = false
      pagesLoaded.current = 0
    }
    if (placed.current || !el || !chat) return
    // A window, or a jump on its way: the chat stands at that message, not at the line.
    if (inWindow || (jump && jumped.current !== jump.seq)) {
      placed.current = true
      return
    }
    if (!unreadLine) {
      placed.current = true
      // Only after a notification click: a chat that opens stands at the end by itself.
      if (el.scrollTop < 0) {
        ownScroll.current = true
        el.scrollTop = 0
        updateAtBottom(true)
        setShowJump(false)
        markVisibleRead(room.token)
      }
      return
    }
    if (!chat.synced || chat.loadingOlder) return
    const wanted = counting.length > 0 && !lineCovered && chat.hasMore && chat.oldestId != null
    if (wanted && pagesLoaded.current < MAX_PAGES_TO_LINE) {
      pagesLoaded.current += 1
      void loadOlder(room.token)
      return
    }
    placed.current = true
    const line = el.querySelector<HTMLElement>('.unread-line')
    // Never further than the end (scrollTop 0). Not reached after all pages: the oldest loaded
    // message on top. Nothing foreign among the new ones: the end, as without a line.
    const target = line
      ? Math.min(0, el.scrollTop + line.getBoundingClientRect().top - el.getBoundingClientRect().top - el.clientHeight / 4)
      : lineCovered || counting.length === 0
        ? 0
        : el.clientHeight - el.scrollHeight
    if (Math.abs(target - el.scrollTop) >= 1) {
      ownScroll.current = true
      el.scrollTop = target
    }
    const bottom = atEnd(el)
    updateAtBottom(bottom)
    setShowJump(el.scrollTop < -el.clientHeight / 2)
    if (bottom) markVisibleRead(room.token)
  }, [chat, unreadLine, lineCovered, room.token, reopened, inWindow, jump])

  useEffect(() => () => window.clearTimeout(datePillTimer.current), [])

  /** Floating date of the topmost visible message while scrolling, never at the end. */
  function showDatePill(el: HTMLElement, bottom: boolean) {
    window.clearTimeout(datePillTimer.current)
    const row = bottom ? null : topRow(el)
    if (!row) {
      setDatePill((p) => (p.shown ? { ...p, shown: false } : p))
      return
    }
    setDatePill({ label: dayLabel(Number(row.dataset.ts)), shown: true })
    datePillTimer.current = window.setTimeout(() => setDatePill((p) => ({ ...p, shown: false })), DATE_PILL_MS)
  }

  // The scroller is column-reverse: scrollTop 0 is the bottom, negative values go up.
  function onScroll() {
    const el = scroller.current
    if (!el || keepEnd(el)) return
    setReacting(null)
    const bottom = atEnd(el)
    updateAtBottom(bottom)
    setShowJump(el.scrollTop < -el.clientHeight / 2)
    // Our own positioning when opening, and the shift by a new message while scrolled up,
    // are no scrolling by the user.
    const grew = el.scrollHeight !== lastHeight.current
    lastHeight.current = el.scrollHeight
    if (ownScroll.current) ownScroll.current = false
    else if (!grew || bottom) showDatePill(el, bottom)
    if (el.scrollHeight - el.clientHeight + el.scrollTop < 400) void loadOlder(room.token)
    if (windowRef.current && el.scrollTop > -NEWER_AHEAD) void loadNewer(room.token)
  }

  // The end stays the end when the input area grows (reply or edit bar, a longer text): the
  // browser would keep the visible messages in place and hide the last one behind the bar.
  // Its shift comes as a scroll event before or after the resize, so both ask here.
  function keepEnd(el: HTMLElement): boolean {
    const shrunk = el.clientHeight < viewHeight.current
    viewHeight.current = el.clientHeight
    if (!shrunk || !placed.current || !atBottomRef.current || el.scrollTop >= 0) return false
    ownScroll.current = true
    el.scrollTop = 0
    return true
  }
  useEffect(() => {
    const el = scroller.current
    if (!el) return
    const observer = new ResizeObserver(() => keepEnd(el))
    observer.observe(el)
    return () => observer.disconnect()
  }, [])

  // Fill the viewport when the first page is shorter than the window.
  useLayoutEffect(() => {
    const el = scroller.current
    if (el && chat && !chat.loading && chat.hasMore && el.scrollHeight <= el.clientHeight) void loadOlder(room.token)
  }, [chat, room.token])

  function scrollToBottom() {
    const el = scroller.current
    if (!el) return
    // Far away (older pages loaded): jump to one screen before the end, animate only the rest.
    if (el.scrollTop < -2 * el.clientHeight) el.scrollTo({ top: -el.clientHeight, behavior: 'instant' })
    el.scrollTo({ top: 0, behavior: 'smooth' })
  }

  const jumpTo = useCallback((id: number) => void showMessage(room.token, id), [room.token])
  const openMenu = useCallback((msg: UiMessage, x: number, y: number) => setMenu({ msg, x, y }), [])
  const closeMenu = useCallback(() => setMenu(null), [])
  const closeViewer = useCallback(() => setViewer(null), [])
  const closeReacting = useCallback(() => setReacting(null), [])
  const closeWhoReacted = useCallback(() => setWhoReacted(null), [])
  const openWhoReacted = useCallback((m: UiMessage) => setWhoReacted(m.id), [])
  const openReacting = useCallback((msg: UiMessage, bubble: HTMLElement, toggle: HTMLElement | null, full = false) => {
    setReacting((cur) =>
      cur && cur.msg.id === msg.id && !full ? null : { msg, anchor: bubble.getBoundingClientRect(), toggle, full },
    )
  }, [])

  /** The context menu's top row (docs/chat.md „Reaktionen“). */
  function menuHeader(m: UiMessage) {
    if (!reactable(room.canReact, m)) return undefined
    return (
      <ReactionBar
        msg={m}
        onPick={(emoji) => {
          setMenu(null)
          void react(room.token, m.id, emoji)
        }}
        onMore={() => {
          setMenu(null)
          const bubble = scroller.current?.querySelector<HTMLElement>(`.bubble[data-id="${m.id}"]`)
          if (bubble) openReacting(m, bubble, null, true)
        }}
      />
    )
  }

  function menuItems(m: UiMessage): MenuItem[] {
    const items: MenuItem[] = []
    if (m.status) {
      items.push({ label: 'Erneut senden', onSelect: () => retry(m) })
      items.push({ label: 'Löschen', onSelect: () => discard(m), danger: true })
      return items
    }
    if (m.kind === 'deleted') return items
    if (!room.readOnly) items.push({ label: 'Antworten', onSelect: () => reply(m) })
    if (canEdit(room, m)) items.push({ label: 'Bearbeiten', onSelect: () => void startEdit(m) })
    const place = m.location
    if (place) {
      // Name and, in the second line, the link (docs/attachments.md).
      items.push({
        label: 'Kopieren',
        onSelect: () => void navigator.clipboard.writeText(`${locationName(place)}\n${locationLink(place)}`),
      })
      items.push({ label: 'Im Browser öffnen', onSelect: () => window.open(locationLink(place), '_blank') })
    } else if (m.text) items.push({ label: 'Text kopieren', onSelect: () => void navigator.clipboard.writeText(m.text) })
    if (m.attachment?.link) {
      if (isImageAttachment(m.attachment)) items.push({ label: 'Bild öffnen', onSelect: () => setViewer(m) })
      items.push({ label: 'Im Browser öffnen', onSelect: () => window.open(m.attachment!.link, '_blank') })
    }
    if (canDelete(room, m)) items.push({ label: 'Löschen', onSelect: () => setDeleting(m), danger: true })
    return items
  }

  function isFileDrag(e: DragEvent) {
    return !room.readOnly && !editing && e.dataTransfer.types.includes('Files')
  }

  const lastCommonRead = Math.max(chat?.lastCommonRead ?? 0, room.lastCommonReadMessage)

  return (
    <section
      className="chat"
      onDragEnter={(e) => isFileDrag(e) && setDropping(true)}
      onDragOver={(e) => {
        if (!isFileDrag(e)) return
        e.preventDefault()
        e.dataTransfer.dropEffect = 'copy'
      }}
      onDragLeave={(e) => {
        if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setDropping(false)
      }}
      onDrop={(e) => {
        setDropping(false)
        if (!isFileDrag(e)) return
        e.preventDefault()
        const files = [...e.dataTransfer.files]
        if (files.length) addFiles.current?.(files)
      }}
    >
      <header className="chat-header">
        {isGroupRoom(room) ? (
          <button
            className="chat-header-main"
            onClick={() => openGroupInfo(room.token)}
            title="Gruppeneinstellungen"
            aria-label={`${room.name}, Gruppeneinstellungen`}
          >
            <ChatTitle room={room} />
          </button>
        ) : (
          <div className="chat-header-main">
            <ChatTitle room={room} />
          </div>
        )}
        <div className="chat-header-actions">
          <button className="header-icon-btn" onClick={() => focusSearch(true)} title="Im Chat suchen" aria-label="Im Chat suchen">
            <svg viewBox="0 0 24 24" width="20" height="20" aria-hidden fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round">
              <circle cx="10.5" cy="10.5" r="6" />
              <path d="M15 15l4.5 4.5" />
            </svg>
          </button>
          <CallButton room={room} />
        </div>
      </header>
      <RecordingBanners token={room.token} />

      <div className="timeline-wrap">
        <div className="timeline" ref={scroller} onScroll={onScroll}>
          <div className="timeline-inner">
            {chat?.loadingOlder && <div className="spinner small" />}
            {!chat?.hasMore && messages.length > 0 && <div className="timeline-start">Beginn der Unterhaltung</div>}
            {messages.map((m, i) => (
              <MessageRow
                key={m.referenceId && m.status ? m.referenceId : m.id}
                msg={m}
                prev={messages[i - 1]}
                next={messages[i + 1]}
                isGroup={isGroup}
                unread={m.id === lineAt ? unreadCount : 0}
                read={m.isOwn && !m.status && m.id <= lastCommonRead}
                canReply={!room.readOnly}
                canReact={room.canReact}
                token={room.token}
                onReply={reply}
                onReact={openReacting}
                onWhoReacted={openWhoReacted}
                onMenu={openMenu}
                onOpenImage={setViewer}
                onJump={jumpTo}
                marks={marks}
              />
            ))}
            {chat?.loading && <div className="spinner" />}
          </div>
        </div>
        <div className={`date-pill${datePill.shown ? ' shown' : ''}`} aria-hidden="true">
          {datePill.label}
        </div>
        <JumpButton
          shown={showJump || inWindow}
          unread={inWindow ? Math.max(room.unreadMessages, heldCount) : room.unreadMessages}
          onJump={() => (inWindow ? void showEnd(room.token) : scrollToBottom())}
        />
      </div>

      {room.readOnly ? (
        <div className="composer readonly muted">Diese Unterhaltung ist schreibgeschützt.</div>
      ) : (
        <Composer
          ref={composerRef}
          token={room.token}
          replyTo={replyTo}
          editing={editing}
          onEndEdit={endEdit}
          onEditLast={editLast}
          addFilesRef={addFiles}
          onCancelReply={() => setReplyTo(null)}
          onSent={() => {
            setReplyTo(null)
            // In a window the store leads to the end of the chat (`showQueued`).
            if (!windowRef.current) scroller.current?.scrollTo({ top: 0 })
          }}
        />
      )}

      {dropping && (
        <div className="drop-overlay">
          <div>Zum Senden hier ablegen</div>
        </div>
      )}
      {menu && (
        <MessageMenu
          x={menu.x}
          y={menu.y}
          header={menuHeader(menu.msg)}
          items={menuItems(menu.msg)}
          onClose={closeMenu}
        />
      )}
      {reacting && (
        <ReactionPopover
          key={`${reacting.msg.id}/${reacting.full}`}
          token={room.token}
          msg={messages.find((m) => m.id === reacting.msg.id) ?? reacting.msg}
          anchor={reacting.anchor}
          toggle={reacting.toggle}
          startFull={reacting.full}
          onClose={closeReacting}
        />
      )}
      {deleting && (
        <ConfirmDialog
          title="Nachricht löschen?"
          text={deleteText(deleting)}
          confirm="Löschen"
          danger
          busy={deleteBusy}
          onCancel={() => setDeleting(null)}
          onConfirm={() => void confirmDelete(deleting)}
        />
      )}
      {whoReacted !== null && <WhoReacted token={room.token} messageId={whoReacted} onClose={closeWhoReacted} />}
      {viewer && <ImageViewer msg={viewer} onClose={closeViewer} />}
    </section>
  )
})

interface RowProps {
  msg: UiMessage
  prev?: UiMessage
  next?: UiMessage
  isGroup: boolean
  /** Above this message: the unread line with this count (0: none). */
  unread: number
  read: boolean
  canReply: boolean
  canReact: boolean
  token: string
  onReply: (m: UiMessage) => void
  onReact: (m: UiMessage, bubble: HTMLElement, toggle: HTMLElement | null) => void
  onWhoReacted: (m: UiMessage) => void
  onMenu: (m: UiMessage, x: number, y: number) => void
  onOpenImage: (m: UiMessage) => void
  /** A click on the quote: to the quoted message. */
  onJump: (id: number) => void
  /** Search words to mark in text and caption. */
  marks: Marks
}

function continues(a: UiMessage | undefined, b: UiMessage | undefined): boolean {
  if (!a || !b || a.kind === 'system' || b.kind === 'system') return false
  return (
    a.actorId === b.actorId &&
    a.actorType === b.actorType &&
    Math.abs(b.timestamp - a.timestamp) <= GROUP_WINDOW_S &&
    sameDay(a.timestamp, b.timestamp)
  )
}

const MessageRow = memo(function MessageRow({
  msg,
  prev,
  next,
  isGroup,
  unread,
  read,
  canReply,
  canReact,
  token,
  onReply,
  onReact,
  onWhoReacted,
  onMenu,
  onOpenImage,
  onJump,
  marks,
}: RowProps) {
  const showDay = !prev || !sameDay(prev.timestamp, msg.timestamp)
  const line = unread > 0 && (
    <div className="unread-line" role="separator" aria-label={unreadText(unread)}>
      <span aria-hidden="true">{unreadText(unread)}</span>
    </div>
  )
  const day = showDay && (
    <div className="day" role="heading" aria-level={3}>
      {dayLabel(msg.timestamp)}
    </div>
  )

  if (msg.kind === 'system') {
    return (
      <>
        {day}
        <div className="system-msg" data-ts={msg.timestamp} title={fullTime(msg.timestamp)}>
          {msg.text}
        </div>
      </>
    )
  }

  const first = !continues(prev, msg)
  const last = !continues(msg, next)
  const own = msg.isOwn
  const deleted = msg.kind === 'deleted'
  const attachment = deleted ? null : msg.attachment
  const image = isImageAttachment(attachment) ? attachment : null
  // A location is drawn as a tile, its text is the name once more.
  const location = deleted ? null : msg.location
  const jumbo = msg.kind === 'text' && !msg.replyTo && !attachment && !location && isJumboEmoji(msg.text)
  const reactions = Object.entries(msg.reactions)
  const showAuthor = isGroup && !own && first
  const hasText = !deleted && !location && msg.text.trim() !== ''
  const showMeta = last || !!msg.status
  // Time on the photo when nothing follows it inside the bubble.
  const metaOnImage = !!image && !hasText
  const cls = ['msg', own ? 'own' : 'other', first ? 'first' : '', last ? 'last' : '', reactions.length ? 'reacted' : '']
    .filter(Boolean)
    .join(' ')
  const bubbleCls = [
    'bubble',
    jumbo && 'jumbo',
    deleted && 'deleted',
    image && 'media',
    image && !showAuthor && !msg.replyTo && 'media-top',
    metaOnImage && 'media-bottom',
    msg.status === 'failed' && 'failed',
  ]
    .filter(Boolean)
    .join(' ')

  const meta = showMeta && (
    <span className={`meta${msg.status === 'failed' ? ' error' : ''}${metaOnImage ? ' on-image' : ''}`}>
      {msg.edited && 'bearbeitet · '}
      {statusText(msg) ?? messageTime(msg.timestamp)}
      {own && !msg.status && <Ticks read={read} />}
    </span>
  )

  // Signal: a voice message shows its time in the player's footer row.
  const metaInVoice = !!attachment && !image && isAudioAttachment(msg) && !hasText && !deleted

  const quoted = msg.replyTo ? withAttachment(msg.replyTo.text, msg.replyTo.attachment, msg.replyTo.location) : ''
  const replyable = canReply && msg.kind === 'text' && !msg.status
  const canReactHere = reactable(canReact, msg)

  return (
    <>
      {line}
      {day}
      <div className={cls} data-ts={msg.timestamp}>
        {isGroup && !own && (
          <div className="msg-avatar">{last && <Avatar name={msg.author} userId={msg.actorType === 'users' ? msg.actorId : undefined} size={28} />}</div>
        )}
        <div className="msg-col">
          <div
            className={bubbleCls}
            data-id={msg.id}
            title={fullTime(msg.timestamp)}
            onDoubleClick={() => replyable && !image && onReply(msg)}
            onClick={() => msg.status === 'failed' && retry(msg)}
            onContextMenu={(e) => {
              e.preventDefault()
              onMenu(msg, e.clientX, e.clientY)
            }}
          >
            {showAuthor && (
              <div className="author" style={{ color: colorFor(msg.author) }}>
                {msg.author}
              </div>
            )}
            {msg.replyTo && (
              <div
                className={`quote${msg.replyTo.isOwn ? ' own' : ''}${quoted ? ' jumps' : ''}`}
                // docs/search.md „Zitat antippen“; the quote of a deleted message does nothing.
                onClick={
                  quoted
                    ? (e) => {
                        e.stopPropagation()
                        onJump(msg.replyTo!.id)
                      }
                    : undefined
                }
                onDoubleClick={(e) => e.stopPropagation()}
              >
                <div className="quote-author">{msg.replyTo.isOwn ? 'Du' : msg.replyTo.author}</div>
                <div className="quote-text">{quoted || 'Nachricht gelöscht'}</div>
              </div>
            )}
            {image && (
              <ChatImage msg={msg} onOpen={() => onOpenImage(msg)}>
                {metaOnImage && meta}
              </ChatImage>
            )}
            {(!image || hasText) && (
              <div className="text">
                {attachment &&
                  !image &&
                  (isAudioAttachment(msg) ? (
                    <VoiceMessage msg={msg} meta={metaInVoice ? meta : undefined} />
                  ) : (
                    <FileRow msg={msg} />
                  ))}
                {location && <LocationRow location={location} />}
                {deleted ? (
                  <em>Diese Nachricht wurde gelöscht.</em>
                ) : location ? null : msg.markdown ? (
                  <Markdown text={msg.text} marks={marks} />
                ) : (
                  splitLinks(msg.text).map((part, i) =>
                    part.href ? (
                      <a key={i} href={part.href} target="_blank" rel="noreferrer">
                        {marked(part.text, marks, i)}
                      </a>
                    ) : (
                      <Fragment key={i}>{marked(part.text, marks, i)}</Fragment>
                    ),
                  )
                )}
                {!metaOnImage && !metaInVoice && meta}
              </div>
            )}
          </div>
          <ReactionPills token={token} msg={msg} onOpen={onWhoReacted} />
        </div>
        <div className="msg-actions">
          {canReactHere && (
            <button
              className="msg-action"
              aria-label="Reagieren"
              title="Reagieren"
              onClick={(e) => {
                const bubble = e.currentTarget.closest('.msg')?.querySelector<HTMLElement>('.bubble')
                if (bubble) onReact(msg, bubble, e.currentTarget)
              }}
            >
              <svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true">
                <circle cx="12" cy="12" r="8.5" fill="none" stroke="currentColor" strokeWidth="1.6" />
                <circle cx="9" cy="10" r="1.2" fill="currentColor" />
                <circle cx="15" cy="10" r="1.2" fill="currentColor" />
                <path d="M8.3 14a4.3 4.3 0 0 0 7.4 0" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" />
              </svg>
            </button>
          )}
          {replyable && (
            <button className="msg-action" aria-label="Antworten" title="Antworten" onClick={() => onReply(msg)}>
              <svg viewBox="0 0 24 24" width="18" height="18">
                <path fill="currentColor" d="M10 9V5l-7 7 7 7v-4.1c5 0 8.5 1.6 11 5.1-1-5-4-10-11-11z" />
              </svg>
            </button>
          )}
          {!deleted && (
            <button
              className="msg-action"
              aria-label="Mehr"
              title="Mehr"
              onClick={(e) => {
                const r = e.currentTarget.getBoundingClientRect()
                onMenu(msg, r.left, r.bottom + 4)
              }}
            >
              <svg viewBox="0 0 24 24" width="18" height="18">
                <circle cx="5" cy="12" r="1.8" fill="currentColor" />
                <circle cx="12" cy="12" r="1.8" fill="currentColor" />
                <circle cx="19" cy="12" r="1.8" fill="currentColor" />
              </svg>
            </button>
          )}
        </div>
      </div>
    </>
  )
})

function failureText(e: SendError | null | undefined): string {
  switch (e) {
    case 'forbidden':
      return 'keine Berechtigung'
    case 'notFound':
      return 'Unterhaltung nicht gefunden'
    case 'tooLarge':
      return 'Datei zu groß'
    case 'quotaExceeded':
      return 'Speicher voll'
    case 'fileMissing':
      return 'Datei fehlt'
    default:
      return 'Klicken zum Wiederholen'
  }
}

/** Outbox state in place of the time, like the Android app. */
function statusText(m: UiMessage): string | null {
  switch (m.status) {
    case 'sending':
      return 'Wird gesendet …'
    case 'waiting':
      return 'Wartet auf Netz …'
    case 'failed':
      return `Nicht gesendet · ${failureText(m.error)}`
    default:
      return null
  }
}

function Ticks({ read }: { read: boolean }) {
  return (
    <svg className="status" viewBox="0 0 20 14" width={read ? 18 : 12} height="12" aria-label={read ? 'Gelesen' : 'Gesendet'}>
      {read ? (
        <>
          <path d="M1.5 7.5l3.5 3.5 7-8" fill="none" stroke="currentColor" strokeWidth="1.7" />
          <path d="M8 11l7-8" fill="none" stroke="currentColor" strokeWidth="1.7" />
        </>
      ) : (
        <path d="M2 7.5l5 4.5 11-10" fill="none" stroke="currentColor" strokeWidth="2" />
      )}
    </svg>
  )
}
