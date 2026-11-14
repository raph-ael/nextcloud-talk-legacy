import { forwardRef, memo, useCallback, useEffect, useLayoutEffect, useRef, useState, type DragEvent } from 'react'
import { withAttachment, type RoomView, type SendError } from '@shared/core'
import {
  discard,
  loadOlder,
  markVisibleRead,
  openGroupInfo,
  retry,
  countsAsUnread,
  setChatAtBottom,
  useStore,
  type UiMessage,
} from '../store'
import { isGroupRoom } from './GroupInfo'
import { colorFor, dayLabel, fullTime, isJumboEmoji, messageTime, sameDay, splitLinks } from '../format'
import { Markdown, plain } from './Markdown'
import { Avatar } from './Avatar'
import { ChatImage, FileRow, isImageAttachment } from './ChatMedia'
import { Composer } from './Composer'
import { VoiceMessage } from './Voice'
import { isAudioAttachment } from '../voice'
import { ImageViewer } from './ImageViewer'
import { MessageMenu, type MenuItem } from './MessageMenu'
import { OfflineBanner } from './Sidebar'
import { react, reactable, ReactionBar, ReactionPills, ReactionPopover, WhoReacted } from './Reactions'
import { CallButton } from '../call/CallScreen'
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
  return (
    <>
      <Avatar key={room.avatarVersion} name={room.name} token={room.token} version={room.avatarVersion} size={32} />
      <div className="chat-title">
        <h2>{room.name}</h2>
        {room.description && <span className="muted ellipsis">{plain(room.description)}</span>}
      </div>
    </>
  )
}

/** Messages of the same author within this window form one visual group. */
const GROUP_WINDOW_S = 3 * 60

export const Chat = forwardRef<HTMLTextAreaElement, { room: RoomView }>(function Chat({ room }, composerRef) {
  const chat = useStore((s) => s.chats[room.token])
  const online = useStore((s) => s.online)
  const [replyTo, setReplyTo] = useState<UiMessage | null>(null)
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

  const isGroup = room.roomType !== 1 && room.roomType !== 6
  const messages = chat?.messages ?? []
  const unreadLine = useStore((s) => (s.unreadLine?.token === room.token ? s.unreadLine : null))

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

  function updateAtBottom(value: boolean) {
    atBottomRef.current = value
    setChatAtBottom(value)
    setAtBottom(value)
  }
  useEffect(() => () => setChatAtBottom(true), [])

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
    if (placed.current || !el || !chat) return
    if (!unreadLine) {
      placed.current = true
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
    const bottom = el.scrollTop > -40
    updateAtBottom(bottom)
    setShowJump(el.scrollTop < -el.clientHeight / 2)
    if (bottom) markVisibleRead(room.token)
  }, [chat, unreadLine, lineCovered, room.token])

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
    if (!el) return
    setReacting(null)
    const bottom = el.scrollTop > -40
    updateAtBottom(bottom)
    setShowJump(el.scrollTop < -el.clientHeight / 2)
    // Our own positioning when opening, and the shift by a new message while scrolled up,
    // are no scrolling by the user.
    const grew = el.scrollHeight !== lastHeight.current
    lastHeight.current = el.scrollHeight
    if (ownScroll.current) ownScroll.current = false
    else if (!grew || bottom) showDatePill(el, bottom)
    if (el.scrollHeight - el.clientHeight + el.scrollTop < 400) void loadOlder(room.token)
  }

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
    if (!room.readOnly) items.push({ label: 'Antworten', onSelect: () => setReplyTo(m) })
    if (m.text) items.push({ label: 'Text kopieren', onSelect: () => void navigator.clipboard.writeText(m.text) })
    if (m.attachment?.link) {
      if (isImageAttachment(m.attachment)) items.push({ label: 'Bild öffnen', onSelect: () => setViewer(m) })
      items.push({ label: 'Im Browser öffnen', onSelect: () => window.open(m.attachment!.link, '_blank') })
    }
    return items
  }

  function isFileDrag(e: DragEvent) {
    return !room.readOnly && e.dataTransfer.types.includes('Files')
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
          <CallButton room={room} />
        </div>
      </header>
      {!online && <OfflineBanner />}
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
                onReply={setReplyTo}
                onReact={openReacting}
                onWhoReacted={openWhoReacted}
                onMenu={openMenu}
                onOpenImage={setViewer}
              />
            ))}
            {chat?.loading && <div className="spinner" />}
          </div>
        </div>
        <div className={`date-pill${datePill.shown ? ' shown' : ''}`} aria-hidden="true">
          {datePill.label}
        </div>
        <JumpButton shown={showJump} unread={room.unreadMessages} onJump={scrollToBottom} />
      </div>

      {room.readOnly ? (
        <div className="composer readonly muted">Diese Unterhaltung ist schreibgeschützt.</div>
      ) : (
        <Composer
          ref={composerRef}
          token={room.token}
          replyTo={replyTo}
          addFilesRef={addFiles}
          onCancelReply={() => setReplyTo(null)}
          onSent={() => {
            setReplyTo(null)
            scroller.current?.scrollTo({ top: 0 })
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
  const jumbo = msg.kind === 'text' && !msg.replyTo && !attachment && isJumboEmoji(msg.text)
  const reactions = Object.entries(msg.reactions)
  const showAuthor = isGroup && !own && first
  const hasText = !deleted && msg.text.trim() !== ''
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
              <div className={`quote${msg.replyTo.isOwn ? ' own' : ''}`}>
                <div className="quote-author">{msg.replyTo.isOwn ? 'Du' : msg.replyTo.author}</div>
                <div className="quote-text">
                  {withAttachment(msg.replyTo.text, msg.replyTo.attachment) || 'Nachricht gelöscht'}
                </div>
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
                {deleted ? (
                  <em>Diese Nachricht wurde gelöscht.</em>
                ) : msg.markdown ? (
                  <Markdown text={msg.text} />
                ) : (
                  splitLinks(msg.text).map((part, i) =>
                    part.href ? (
                      <a key={i} href={part.href} target="_blank" rel="noreferrer">
                        {part.text}
                      </a>
                    ) : (
                      part.text
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
