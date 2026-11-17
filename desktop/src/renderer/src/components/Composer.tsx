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
import { toast } from '../call/store'
import type { Editing } from './Chat'
import { dropDraft, releaseDraft, stage, type Draft } from '../media'
import {
  cancelRecording,
  clearNotice,
  discardDraft,
  draftKey,
  loadDraft,
  recordingFor,
  sendDraft,
  sendRecording,
  startRecording,
  stopToDraft,
  useVoice,
} from '../voice'
import { DraftBar, MicIcon, RecordingBar } from './Voice'

// Emoji data is large: loaded on first use, not at startup.
const IS_MAC = /Mac/.test(navigator.userAgent)
const VOICE_HINT = IS_MAC ? 'Sprachnachricht aufnehmen (⇧⌘Y)' : 'Sprachnachricht aufnehmen (Strg+Umschalt+Y)'

const EmojiPicker = lazy(() => import('./EmojiPicker').then((m) => ({ default: m.EmojiPicker })))

/**
 * Text and attachments not sent yet, per account and conversation (`draftKey`): kept while
 * switching chats and accounts.
 */
const textDrafts = new Map<string, string>()
const fileDrafts = new Map<string, Draft[]>()

interface Props {
  token: string
  replyTo: UiMessage | null
  onCancelReply: () => void
  /** The message being edited (docs/chat.md „Bearbeiten“): its text is in the field, not the draft. */
  editing: Editing | null
  onEndEdit: () => void
  /** Starts editing the last own message that allows it; `false` when there is none. */
  onEditLast: () => boolean
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
  { token, replyTo, onCancelReply, editing, onEndEdit, onEditLast, onSent, addFilesRef },
  ref,
) {
  // The chat remounts for another account (its key), so the key holds for this composer.
  const [key] = useState(() => draftKey(token))
  const [text, setText] = useState(() => textDrafts.get(key) ?? '')
  const [files, setFiles] = useState<Draft[]>(() => fileDrafts.get(key) ?? [])
  const [staging, setStaging] = useState(0)
  const [error, setError] = useState('')
  const [emoji, setEmoji] = useState(false)
  const [saving, setSaving] = useState(false)
  const editingRef = useRef(editing)
  editingRef.current = editing
  const input = useRef<HTMLTextAreaElement | null>(null)
  const picker = useRef<HTMLInputElement>(null)
  const emojiButton = useRef<HTMLButtonElement>(null)
  const recording = useVoice((s) => recordingFor(s, token))
  const voiceDraft = useVoice((s) => s.drafts[key] ?? null)
  const notice = useVoice((s) => (s.notice?.key === key ? s.notice.text : null))

  useEffect(() => {
    setText(textDrafts.get(key) ?? '')
    setFiles(fileDrafts.get(key) ?? [])
    setError('')
    input.current?.focus()
    void loadDraft(token)
    return () => {
      // Switching chats keeps a running recording as draft of its chat (like Signal).
      void stopToDraft()
      clearNotice()
    }
  }, [token])

  // Esc cancels a recording; Ctrl/Cmd+Shift+Y starts one (Signal Desktop's shortcut).
  useEffect(() => {
    function onKey(e: globalThis.KeyboardEvent) {
      if (recording && e.key === 'Escape') {
        e.preventDefault()
        void cancelRecording()
      } else if ((e.ctrlKey || e.metaKey) && e.shiftKey && e.key.toLowerCase() === 'y') {
        e.preventDefault()
        if (recording) void sendRecording(replyTo).then(onSent)
        else if (!editing && !voiceDraft && !text.trim() && files.length === 0) void startRecording(token)
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [recording, voiceDraft, text, files.length, token, replyTo, onSent, editing])

  // Edit mode: the field takes the message's text with the caret at its end. Draft, staged
  // files, a voice draft and a started reply wait and are back afterwards.
  useEffect(() => {
    setSaving(false)
    if (!editing) {
      setText(textDrafts.get(key) ?? '')
      input.current?.focus()
      return
    }
    void stopToDraft()
    setEmoji(false)
    setText(editing.source)
    const end = editing.source.length
    requestAnimationFrame(() => {
      const el = input.current
      if (!el) return
      el.focus()
      el.setSelectionRange(end, end)
      el.scrollTop = el.scrollHeight
    })
  }, [editing])

  // Esc also cancels when the focus is nowhere (after a click into the timeline); menus,
  // dialogs and the image viewer come first.
  useEffect(() => {
    if (!editing) return
    function onKey(e: globalThis.KeyboardEvent) {
      if (e.key !== 'Escape' || document.activeElement !== document.body) return
      if (document.querySelector('.modal, .menu-layer, .viewer')) return
      e.preventDefault()
      e.stopPropagation()
      onEndEdit()
    }
    window.addEventListener('keydown', onKey, true)
    return () => window.removeEventListener('keydown', onKey, true)
  }, [editing, onEndEdit])
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
    // The edited text never becomes the draft.
    if (editing) return
    if (v) textDrafts.set(key, v)
    else textDrafts.delete(key)
  }

  function updateFiles(f: (list: Draft[]) => Draft[]) {
    setFiles((list) => {
      const next = f(list)
      if (next.length) fileDrafts.set(key, next)
      else fileDrafts.delete(key)
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
  addFilesRef.current = (f) => {
    if (!editingRef.current) void addFiles(f)
  }

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

  // Off while the text is empty or the same as before (both without whitespace at the edges).
  const canSave = !!editing && !saving && text.trim() !== '' && text.trim() !== editing.source.trim()

  async function save() {
    if (!editing || !canSave) return
    setSaving(true)
    const error = await window.talk.editMessage(token, editing.msg.id, text.trim()).catch(() => 'Nachricht nicht geändert')
    // Another message is being edited by now: its mode is not ours to end.
    if (editingRef.current !== editing) return
    setSaving(false)
    if (error) {
      // The mode stays open with the text; the new version comes with `messagesUpdated`.
      toast(error)
      input.current?.focus()
    } else onEndEdit()
  }

  function onKeyDown(e: KeyboardEvent<HTMLTextAreaElement>) {
    if (e.key === 'Enter' && !e.shiftKey && !e.nativeEvent.isComposing) {
      e.preventDefault()
      if (editing) void save()
      else submit()
    } else if (e.key === 'Escape') {
      if (emoji) setEmoji(false)
      else if (editing) onEndEdit()
      else if (replyTo) onCancelReply()
    } else if (
      e.key === 'ArrowUp' &&
      !e.shiftKey &&
      !e.altKey &&
      !e.ctrlKey &&
      !e.metaKey &&
      !editing &&
      text === '' &&
      files.length === 0 &&
      staging === 0
    ) {
      if (onEditLast()) e.preventDefault()
    }
  }

  function onPaste(e: ClipboardEvent<HTMLTextAreaElement>) {
    const pasted = [...e.clipboardData.files]
    if (pasted.length === 0 || editing) return
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
      {editing ? (
        <div className="reply-bar edit-bar" role="group" aria-label="Nachricht bearbeiten">
          <div className="quote">
            <div className="quote-author">
              <svg viewBox="0 0 24 24" width="14" height="14" aria-hidden="true">
                <path
                  fill="currentColor"
                  d="M3 17.3V21h3.7L17.8 9.9l-3.7-3.7L3 17.3zM20.7 7a1 1 0 0 0 0-1.4l-2.3-2.3a1 1 0 0 0-1.4 0l-1.8 1.8 3.7 3.7L20.7 7z"
                />
              </svg>
              Nachricht bearbeiten
            </div>
            <div className="quote-text">{editing.msg.text.trim() || editing.msg.attachment?.name || ''}</div>
          </div>
          <button className="icon-button" aria-label="Bearbeiten abbrechen" title="Bearbeiten abbrechen" onClick={onEndEdit}>
            <CloseIcon />
          </button>
        </div>
      ) : replyTo && (
        <div className="reply-bar">
          <div className={`quote${replyTo.isOwn ? ' own' : ''}`}>
            <div className="quote-author">{replyTo.isOwn ? 'Du' : replyTo.author}</div>
            <div className="quote-text">{withAttachment(replyTo.text, replyTo.attachment, replyTo.location)}</div>
          </div>
          <button className="icon-button" aria-label="Antwort abbrechen" title="Antwort abbrechen" onClick={onCancelReply}>
            <CloseIcon />
          </button>
        </div>
      )}
      {!editing && (files.length > 0 || staging > 0) && (
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
      {(error || notice) && <div className="composer-error">{error || notice}</div>}
      {recording && !editing ? (
        <RecordingBar
          rec={recording}
          onCancel={() => void cancelRecording()}
          onSend={() => {
            void sendRecording(replyTo).then(onSent)
            onCancelReply()
          }}
        />
      ) : voiceDraft && !editing ? (
        <DraftBar
          draft={voiceDraft}
          onDiscard={() => void discardDraft(token)}
          onSend={() => {
            void sendDraft(token, replyTo).then(onSent)
            onCancelReply()
          }}
        />
      ) : (
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
          placeholder={files.length && !editing ? 'Bildunterschrift hinzufügen' : 'Nachricht'}
          value={text}
          readOnly={saving}
          onChange={(e) => updateText(e.target.value)}
          onKeyDown={onKeyDown}
          onPaste={onPaste}
        />
        {!editing && (
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
        )}
        {editing ? (
          <div className="composer-cell">
            <button
              className="send"
              aria-label="Änderung speichern"
              title="Änderung speichern"
              disabled={!canSave}
              onClick={() => void save()}
            >
              <svg viewBox="0 0 24 24" width="20" height="20" aria-hidden="true">
                <path
                  d="M5 12.5l4.5 4.5L19 7.5"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="2.2"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                />
              </svg>
            </button>
          </div>
        ) : canSend ? (
          <div className="composer-cell">
            <button className="send" aria-label="Senden" title="Senden" onClick={submit}>
              <svg viewBox="0 0 24 24" width="20" height="20">
                <path fill="currentColor" d="M3.4 20.4l17.4-7.5c.8-.4.8-1.5 0-1.8L3.4 3.6c-.7-.3-1.4.3-1.2 1l1.7 6.2 9.1 1.2-9.1 1.2-1.7 6.2c-.2.7.5 1.3 1.2 1z" />
              </svg>
            </button>
          </div>
        ) : (
          files.length === 0 &&
          staging === 0 && (
            <div className="composer-cell">
              <button
                className="icon-button"
                aria-label="Sprachnachricht aufnehmen"
                title={VOICE_HINT}
                onClick={() => void startRecording(token)}
              >
                <MicIcon />
              </button>
            </div>
          )
        )}
      </div>
      )}
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
