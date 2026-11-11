import { forwardRef, memo, useCallback, useEffect, useLayoutEffect, useRef, useState, type DragEvent } from 'react'
import { withAttachment, type RoomView, type SendError } from '@shared/core'
import { discard, loadOlder, markVisibleRead, retry, useStore, type UiMessage } from '../store'
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
import { CallButton } from '../call/CallScreen'
import { RecordingBanners } from '../call/RecordingBanners'

/** Messages of the same author within this window form one visual group. */
const GROUP_WINDOW_S = 3 * 60

export const Chat = forwardRef<HTMLTextAreaElement, { room: RoomView }>(function Chat({ room }, composerRef) {
  const chat = useStore((s) => s.chats[room.token])
  const online = useStore((s) => s.online)
  const [replyTo, setReplyTo] = useState<UiMessage | null>(null)
  const [menu, setMenu] = useState<{ msg: UiMessage; x: number; y: number } | null>(null)
  const [viewer, setViewer] = useState<UiMessage | null>(null)
  const [dropping, setDropping] = useState(false)
  const addFiles = useRef<((files: File[]) => void) | null>(null)
  const scroller = useRef<HTMLDivElement>(null)
  const [atBottom, setAtBottom] = useState(true)

  const isGroup = room.roomType !== 1 && room.roomType !== 6
  const messages = chat?.messages ?? []

  useEffect(() => setReplyTo(null), [room.token])

  // Read marker: whenever new messages are visible in the focused window.
  useEffect(() => {
    if (atBottom) markVisibleRead(room.token)
  }, [room.token, messages.length, atBottom, room.unreadMessages])
  useEffect(() => {
    const onFocus = () => markVisibleRead(room.token)
    window.addEventListener('focus', onFocus)
    return () => window.removeEventListener('focus', onFocus)
  }, [room.token])

  // The scroller is column-reverse: scrollTop 0 is the bottom, negative values go up.
  function onScroll() {
    const el = scroller.current
    if (!el) return
    setAtBottom(el.scrollTop > -40)
    if (el.scrollHeight - el.clientHeight + el.scrollTop < 400) void loadOlder(room.token)
  }

  // Fill the viewport when the first page is shorter than the window.
  useLayoutEffect(() => {
    const el = scroller.current
    if (el && chat && !chat.loading && chat.hasMore && el.scrollHeight <= el.clientHeight) void loadOlder(room.token)
  }, [chat, room.token])

  function scrollToBottom() {
    scroller.current?.scrollTo({ top: 0, behavior: 'smooth' })
  }

  const openMenu = useCallback((msg: UiMessage, x: number, y: number) => setMenu({ msg, x, y }), [])
  const closeMenu = useCallback(() => setMenu(null), [])
  const closeViewer = useCallback(() => setViewer(null), [])

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
        <Avatar name={room.name} token={room.token} version={room.avatarVersion} size={32} />
        <div className="chat-title">
          <h2>{room.name}</h2>
          {room.description && <span className="muted ellipsis">{plain(room.description)}</span>}
        </div>
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
                read={m.isOwn && !m.status && m.id <= lastCommonRead}
                canReply={!room.readOnly}
                onReply={setReplyTo}
                onMenu={openMenu}
                onOpenImage={setViewer}
              />
            ))}
            {chat?.loading && <div className="spinner" />}
          </div>
        </div>
        {!atBottom && (
          <button className="jump" onClick={scrollToBottom} aria-label="Zum Ende springen" title="Zum Ende springen">
            <svg viewBox="0 0 24 24" width="20" height="20">
              <path fill="currentColor" d="M12 16.5l-6-6 1.4-1.4 4.6 4.6 4.6-4.6L18 10.5z" />
            </svg>
          </button>
        )}
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
      {menu && <MessageMenu x={menu.x} y={menu.y} items={menuItems(menu.msg)} onClose={closeMenu} />}
      {viewer && <ImageViewer msg={viewer} onClose={closeViewer} />}
    </section>
  )
})

interface RowProps {
  msg: UiMessage
  prev?: UiMessage
  next?: UiMessage
  isGroup: boolean
  read: boolean
  canReply: boolean
  onReply: (m: UiMessage) => void
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
  read,
  canReply,
  onReply,
  onMenu,
  onOpenImage,
}: RowProps) {
  const showDay = !prev || !sameDay(prev.timestamp, msg.timestamp)
  const day = showDay && (
    <div className="day">
      <span>{dayLabel(msg.timestamp)}</span>
    </div>
  )

  if (msg.kind === 'system') {
    return (
      <>
        {day}
        <div className="system-msg" title={fullTime(msg.timestamp)}>
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

  return (
    <>
      {day}
      <div className={cls}>
        {isGroup && !own && (
          <div className="msg-avatar">{last && <Avatar name={msg.author} size={28} />}</div>
        )}
        <div className="msg-col">
          <div
            className={bubbleCls}
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
          {reactions.length > 0 && (
            <div className="reactions">
              {reactions.map(([emoji, count]) => (
                <span key={emoji} className={msg.reactionsSelf.includes(emoji) ? 'mine' : ''}>
                  {emoji}
                  {count > 1 && <small>{count}</small>}
                </span>
              ))}
            </div>
          )}
        </div>
        <div className="msg-actions">
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
