import {
  forwardRef,
  lazy,
  Suspense,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type ClipboardEvent,
  type KeyboardEvent,
  type MutableRefObject,
} from 'react'
import { withAttachment } from '@shared/core'
import { sendFiles, sendMessage, type UiMessage } from '../store'
import { dropDraft, releaseDraft, stage, type Draft } from '../media'

// Emoji data is large: loaded on first use, not at startup.
const EmojiPicker = lazy(() => import('./EmojiPicker').then((m) => ({ default: m.EmojiPicker })))

/** Text and attachments not sent yet, per conversation (kept while switching chats). */
const textDrafts = new Map<string, string>()
const fileDrafts = new Map<string, Draft[]>()

interface Props {
  token: string
  replyTo: UiMessage | null
  onCancelReply: () => void
  onSent: () => void
  /** Set by the composer: lets the chat hand over dropped files. */
  addFilesRef: MutableRefObject<((files: File[]) => void) | null>
}

/**
 * Signal Desktop's input row: [emoji] [text pill] [attach] and a send button once there is
 * something to send. Picked, dropped and pasted files appear in a rail above the input;
 * the text becomes their caption.
 */
export const Composer = forwardRef<HTMLTextAreaElement, Props>(function Composer(
  { token, replyTo, onCancelReply, onSent, addFilesRef },
  ref,
) {
  const [text, setText] = useState(() => textDrafts.get(token) ?? '')
  const [files, setFiles] = useState<Draft[]>(() => fileDrafts.get(token) ?? [])
  const [staging, setStaging] = useState(0)
  const [error, setError] = useState('')
  const [emoji, setEmoji] = useState(false)
  const input = useRef<HTMLTextAreaElement | null>(null)
  const picker = useRef<HTMLInputElement>(null)
  const emojiButton = useRef<HTMLButtonElement>(null)

  useEffect(() => {
    setText(textDrafts.get(token) ?? '')
    setFiles(fileDrafts.get(token) ?? [])
    setError('')
    input.current?.focus()
  }, [token])
  useEffect(() => {
    if (replyTo) input.current?.focus()
  }, [replyTo])

  // Auto-grow up to the CSS max-height, then scroll (like Signal).
  useLayoutEffect(() => {
    const el = input.current
    if (!el) return
    el.style.height = 'auto'
    const height = el.scrollHeight + el.offsetHeight - el.clientHeight
    el.style.height = `${height}px`
    el.style.overflowY = height > el.offsetHeight ? 'auto' : 'hidden'
  }, [text])

  function updateText(v: string) {
    setText(v)
    if (v) textDrafts.set(token, v)
    else textDrafts.delete(token)
  }

  function updateFiles(f: (list: Draft[]) => Draft[]) {
    setFiles((list) => {
      const next = f(list)
      if (next.length) fileDrafts.set(token, next)
      else fileDrafts.delete(token)
      return next
    })
  }

  async function addFiles(list: File[]) {
    setError('')
    for (const file of list) {
      setStaging((n) => n + 1)
      try {
        const d = await stage(file)
        updateFiles((l) => [...l, d])
      } catch (err) {
        console.warn('attach failed:', err)
        setError(`„${file.name || 'Datei'}“ konnte nicht gelesen werden.`)
      } finally {
        setStaging((n) => n - 1)
      }
    }
    input.current?.focus()
  }
  addFilesRef.current = (f) => void addFiles(f)

  function removeFile(d: Draft) {
    dropDraft(d)
    updateFiles((l) => l.filter((x) => x.key !== d.key))
  }

  const canSend = (text.trim() !== '' || files.length > 0) && staging === 0

  function submit() {
    if (!canSend) return
    const value = text.trim()
    const queued = files
    updateText('')
    updateFiles(() => [])
    setEmoji(false)
    if (queued.length > 0) {
      void sendFiles(
        token,
        queued.map((d) => d.file),
        value,
        replyTo,
      ).finally(() => queued.forEach(releaseDraft))
    } else {
      void sendMessage(token, value, replyTo)
    }
    onSent()
  }

  function onKeyDown(e: KeyboardEvent<HTMLTextAreaElement>) {
    if (e.key === 'Enter' && !e.shiftKey && !e.nativeEvent.isComposing) {
      e.preventDefault()
      submit()
    } else if (e.key === 'Escape') {
      if (emoji) setEmoji(false)
      else if (replyTo) onCancelReply()
    }
  }

  function onPaste(e: ClipboardEvent<HTMLTextAreaElement>) {
    const pasted = [...e.clipboardData.files]
    if (pasted.length === 0) return
    e.preventDefault()
    void addFiles(pasted)
  }

  function insertEmoji(value: string) {
    const el = input.current
    const start = el?.selectionStart ?? text.length
    const end = el?.selectionEnd ?? text.length
    updateText(text.slice(0, start) + value + text.slice(end))
    requestAnimationFrame(() => {
      el?.focus()
      el?.setSelectionRange(start + value.length, start + value.length)
    })
  }

  return (
    <div className="composer">
      {replyTo && (
        <div className="reply-bar">
          <div className={`quote${replyTo.isOwn ? ' own' : ''}`}>
            <div className="quote-author">{replyTo.isOwn ? 'Du' : replyTo.author}</div>
            <div className="quote-text">{withAttachment(replyTo.text, replyTo.attachment)}</div>
          </div>
          <button className="icon-button" aria-label="Antwort abbrechen" title="Antwort abbrechen" onClick={onCancelReply}>
            <CloseIcon />
          </button>
        </div>
      )}
      {(files.length > 0 || staging > 0) && (
        <div className="attachment-rail">
          {files.map((d) => (
            <div key={d.key} className="staged" title={d.file.name}>
              {d.thumb ? (
                <img src={d.thumb} alt="" draggable={false} />
              ) : (
                <div className="staged-file">
                  <span className="staged-ext">{d.file.name.split('.').pop()?.slice(0, 4) || 'Datei'}</span>
                  <span className="staged-name">{d.file.name}</span>
                </div>
              )}
              <button className="staged-remove" aria-label="Entfernen" title="Entfernen" onClick={() => removeFile(d)}>
                <CloseIcon size={14} />
              </button>
            </div>
          ))}
          {staging > 0 && (
            <div className="staged loading">
              <div className="spinner small" />
            </div>
          )}
        </div>
      )}
      {error && <div className="composer-error">{error}</div>}
      <div className="composer-row">
        <div className="composer-cell emoji-anchor">
          <button
            ref={emojiButton}
            className={`icon-button${emoji ? ' active' : ''}`}
            aria-label="Emoji"
            title="Emoji"
            onClick={() => setEmoji((v) => !v)}
          >
            <svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" strokeWidth="1.6">
              <circle cx="12" cy="12" r="9" />
              <path d="M8.5 14.5c.9 1.2 2.1 1.8 3.5 1.8s2.6-.6 3.5-1.8" strokeLinecap="round" />
              <circle cx="9" cy="10" r="1.1" fill="currentColor" stroke="none" />
              <circle cx="15" cy="10" r="1.1" fill="currentColor" stroke="none" />
            </svg>
          </button>
          {emoji && (
            <Suspense fallback={null}>
              <EmojiPicker anchor={emojiButton} onPick={insertEmoji} onClose={() => setEmoji(false)} />
            </Suspense>
          )}
        </div>
        <textarea
          ref={(el) => {
            input.current = el
            if (typeof ref === 'function') ref(el)
            else if (ref) ref.current = el
          }}
          className="composer-input"
          rows={1}
          placeholder={files.length ? 'Bildunterschrift hinzufügen' : 'Nachricht'}
          value={text}
          onChange={(e) => updateText(e.target.value)}
          onKeyDown={onKeyDown}
          onPaste={onPaste}
        />
        <div className="composer-cell">
          <button className="icon-button" aria-label="Datei anhängen" title="Datei anhängen" onClick={() => picker.current?.click()}>
            <svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round">
              <path d="M20 11.5l-7.8 7.8a5 5 0 0 1-7.1-7.1l8.1-8.1a3.3 3.3 0 0 1 4.7 4.7l-8.1 8.1a1.7 1.7 0 0 1-2.4-2.4l7.4-7.4" />
            </svg>
          </button>
          <input
            ref={picker}
            type="file"
            multiple
            hidden
            onChange={(e) => {
              const picked = [...(e.target.files ?? [])]
              e.target.value = ''
              if (picked.length) void addFiles(picked)
            }}
          />
        </div>
        {canSend && (
          <div className="composer-cell">
            <button className="send" aria-label="Senden" title="Senden" onClick={submit}>
              <svg viewBox="0 0 24 24" width="20" height="20">
                <path fill="currentColor" d="M3.4 20.4l17.4-7.5c.8-.4.8-1.5 0-1.8L3.4 3.6c-.7-.3-1.4.3-1.2 1l1.7 6.2 9.1 1.2-9.1 1.2-1.7 6.2c-.2.7.5 1.3 1.2 1z" />
              </svg>
            </button>
          </div>
        )}
      </div>
    </div>
  )
})

export function CloseIcon({ size = 18 }: { size?: number }) {
  return (
    <svg viewBox="0 0 24 24" width={size} height={size}>
      <path fill="currentColor" d="M18.3 5.7 12 12l6.3 6.3-1.4 1.4L12 13.4l-6.3 6.3-1.4-1.4L10.6 12 4.3 5.7l1.4-1.4L12 10.6l6.3-6.3z" />
    </svg>
  )
}
