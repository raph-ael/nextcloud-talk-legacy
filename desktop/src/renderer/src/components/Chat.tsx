import { forwardRef, memo, useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent } from 'react'
import { withAttachment, type RoomView } from '@shared/core'
import { discard, loadOlder, markVisibleRead, retry, sendMessage, useStore, type UiMessage } from '../store'
import { colorFor, dayLabel, fullTime, isJumboEmoji, messageTime, sameDay, splitLinks } from '../format'
import { Markdown, plain } from './Markdown'
import { Avatar } from './Avatar'

/** Messages of the same author within this window form one visual group. */
const GROUP_WINDOW_S = 3 * 60

const drafts = new Map<string, string>()

export const Chat = forwardRef<HTMLTextAreaElement, { room: RoomView }>(function Chat({ room }, composerRef) {
  const chat = useStore((s) => s.chats[room.token])
  const [replyTo, setReplyTo] = useState<UiMessage | null>(null)
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

  const lastCommonRead = Math.max(chat?.lastCommonRead ?? 0, room.lastCommonReadMessage)

  return (
    <section className="chat">
      <header className="chat-header">
        <Avatar name={room.name} token={room.token} version={room.avatarVersion} size={36} />
        <div className="chat-title">
          <h2>{room.name}</h2>
          {room.description && <span className="muted ellipsis">{plain(room.description)}</span>}
        </div>
      </header>

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
              onReply={setReplyTo}
            />
          ))}
          {chat?.loading && <div className="spinner" />}
        </div>
      </div>

      {!atBottom && (
        <button className="jump" onClick={scrollToBottom} aria-label="Zum Ende springen">
          <svg viewBox="0 0 24 24" width="20" height="20">
            <path fill="currentColor" d="M12 16.5l-6-6 1.4-1.4 4.6 4.6 4.6-4.6L18 10.5z" />
          </svg>
        </button>
      )}

      {room.readOnly ? (
        <div className="composer readonly muted">Diese Unterhaltung ist schreibgeschützt.</div>
      ) : (
        <Composer
          ref={composerRef}
          token={room.token}
          replyTo={replyTo}
          onCancelReply={() => setReplyTo(null)}
          onSent={() => {
            setReplyTo(null)
            scroller.current?.scrollTo({ top: 0 })
          }}
        />
      )}
    </section>
  )
})

interface RowProps {
  msg: UiMessage
  prev?: UiMessage
  next?: UiMessage
  isGroup: boolean
  read: boolean
  onReply: (m: UiMessage) => void
}

function continues(a: UiMessage | undefined, b: UiMessage | undefined): boolean {
  if (!a || !b || a.kind === 'system' || b.kind === 'system') return false
  return a.actorId === b.actorId && a.actorType === b.actorType && Math.abs(b.timestamp - a.timestamp) <= GROUP_WINDOW_S && sameDay(a.timestamp, b.timestamp)
}

const MessageRow = memo(function MessageRow({ msg, prev, next, isGroup, read, onReply }: RowProps) {
  const showDay = !prev || !sameDay(prev.timestamp, msg.timestamp)
  const day = showDay && <div className="day"><span>{dayLabel(msg.timestamp)}</span></div>

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
  const jumbo = msg.kind === 'text' && !msg.replyTo && isJumboEmoji(msg.text)
  const cls = ['msg', own ? 'own' : 'other', first ? 'first' : '', last ? 'last' : ''].join(' ')

  return (
    <>
      {day}
      <div className={cls}>
        {isGroup && !own && <div className="msg-avatar">{last && <Avatar name={msg.author} size={28} />}</div>}
        <div
          className={`bubble${jumbo ? ' jumbo' : ''}${msg.kind === 'deleted' ? ' deleted' : ''}${msg.status === 'failed' ? ' failed' : ''}`}
          title={fullTime(msg.timestamp)}
          onDoubleClick={() => msg.kind === 'text' && !msg.status && onReply(msg)}
        >
          {isGroup && !own && first && (
            <div className="author" style={{ color: colorFor(msg.author) }}>
              {msg.author}
            </div>
          )}
          {msg.replyTo && (
            <div className="quote">
              <div className="quote-author">{msg.replyTo.isOwn ? 'Du' : msg.replyTo.author}</div>
              <div className="quote-text">{withAttachment(msg.replyTo.text, msg.replyTo.attachment) || 'Nachricht gelöscht'}</div>
            </div>
          )}
          <div className="text">
            {msg.attachment && (
              <div>
                <a href={msg.attachment.link || '#'} target="_blank" rel="noreferrer">
                  {withAttachment('', msg.attachment)}
                  {msg.attachment.mimetype.startsWith('image/') || msg.attachment.voiceMessage ? ` · ${msg.attachment.name}` : ''}
                </a>
              </div>
            )}
            {msg.kind === 'deleted' ? (
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
            {Object.keys(msg.reactions).length > 0 && (
              <div className="reactions">
                {Object.entries(msg.reactions).map(([emoji, count]) => (
                  <span key={emoji} className={msg.reactionsSelf.includes(emoji) ? 'mine' : ''}>
                    {emoji}
                    {count > 1 && <small>{count}</small>}
                  </span>
                ))}
              </div>
            )}
            {(last || msg.status) && (
              <span className="meta">
                {msg.edited && 'bearbeitet · '}
                {messageTime(msg.timestamp)}
                {own && <Status status={msg.status} read={read} />}
              </span>
            )}
          </div>
        </div>
        {msg.kind === 'text' && !msg.status && (
          <button className="msg-action" aria-label="Antworten" title="Antworten" onClick={() => onReply(msg)}>
            <svg viewBox="0 0 24 24" width="18" height="18">
              <path fill="currentColor" d="M10 9V5l-7 7 7 7v-4.1c5 0 8.5 1.6 11 5.1-1-5-4-10-11-11z" />
            </svg>
          </button>
        )}
      </div>
      {msg.status === 'failed' && (
        <div className="failed-actions">
          Nicht gesendet ·{' '}
          <button className="link" onClick={() => retry(msg.token, msg)}>
            Erneut senden
          </button>{' '}
          ·{' '}
          <button className="link" onClick={() => discard(msg.token, msg)}>
            Verwerfen
          </button>
        </div>
      )}
    </>
  )
})

function Status({ status, read }: { status?: UiMessage['status']; read: boolean }) {
  if (status === 'sending')
    return (
      <svg className="status" viewBox="0 0 16 16" width="13" height="13" aria-label="Wird gesendet">
        <circle cx="8" cy="8" r="6" fill="none" stroke="currentColor" strokeWidth="1.5" />
        <path d="M8 4.5V8l2.5 1.5" fill="none" stroke="currentColor" strokeWidth="1.5" />
      </svg>
    )
  if (status === 'failed') return <span className="status">!</span>
  return (
    <svg className="status" viewBox="0 0 20 14" width="17" height="13" aria-label={read ? 'Gelesen' : 'Gesendet'}>
      <path d="M1.5 7.5l3.5 3.5 7-8" fill="none" stroke="currentColor" strokeWidth="1.7" />
      {read && <path d="M8 11l7-8" fill="none" stroke="currentColor" strokeWidth="1.7" />}
    </svg>
  )
}

interface ComposerProps {
  token: string
  replyTo: UiMessage | null
  onCancelReply: () => void
  onSent: () => void
}

const Composer = forwardRef<HTMLTextAreaElement, ComposerProps>(function Composer(
  { token, replyTo, onCancelReply, onSent },
  ref,
) {
  const [text, setText] = useState(() => drafts.get(token) ?? '')
  const local = useRef<HTMLTextAreaElement | null>(null)

  useEffect(() => {
    setText(drafts.get(token) ?? '')
    local.current?.focus()
  }, [token])
  useEffect(() => {
    if (replyTo) local.current?.focus()
  }, [replyTo])

  // Auto-grow up to the CSS max-height.
  useLayoutEffect(() => {
    const el = local.current
    if (!el) return
    el.style.height = 'auto'
    // border-box: add the borders, otherwise a scrollbar appears for a single line.
    const height = el.scrollHeight + el.offsetHeight - el.clientHeight
    el.style.height = `${height}px`
    el.style.overflowY = height >= window.innerHeight * 0.4 ? 'auto' : 'hidden'
  }, [text])

  function update(v: string) {
    setText(v)
    if (v) drafts.set(token, v)
    else drafts.delete(token)
  }

  function submit() {
    const value = text.trim()
    if (!value) return
    update('')
    void sendMessage(token, value, replyTo)
    onSent()
  }

  function onKeyDown(e: KeyboardEvent<HTMLTextAreaElement>) {
    if (e.key === 'Enter' && !e.shiftKey && !e.nativeEvent.isComposing) {
      e.preventDefault()
      submit()
    } else if (e.key === 'Escape' && replyTo) {
      onCancelReply()
    }
  }

  return (
    <div className="composer">
      {replyTo && (
        <div className="reply-bar">
          <div className="quote">
            <div className="quote-author">{replyTo.isOwn ? 'Du' : replyTo.author}</div>
            <div className="quote-text">{replyTo.text}</div>
          </div>
          <button className="icon-button" aria-label="Antwort abbrechen" onClick={onCancelReply}>
            ✕
          </button>
        </div>
      )}
      <div className="composer-row">
        <textarea
          ref={(el) => {
            local.current = el
            if (typeof ref === 'function') ref(el)
            else if (ref) ref.current = el
          }}
          className="composer-input"
          rows={1}
          placeholder="Nachricht"
          value={text}
          onChange={(e) => update(e.target.value)}
          onKeyDown={onKeyDown}
        />
        <button
          className={`send${text.trim() ? ' visible' : ''}`}
          aria-label="Senden"
          onClick={submit}
          disabled={!text.trim()}
        >
          <svg viewBox="0 0 24 24" width="20" height="20">
            <path fill="currentColor" d="M3.4 20.4l17.4-7.5c.8-.4.8-1.5 0-1.8L3.4 3.6c-.7-.3-1.4.3-1.2 1l1.7 6.2 9.1 1.2-9.1 1.2-1.7 6.2c-.2.7.5 1.3 1.2 1z" />
          </svg>
        </button>
      </div>
    </div>
  )
})
