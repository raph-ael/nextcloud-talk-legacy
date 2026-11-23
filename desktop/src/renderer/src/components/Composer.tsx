import {
  forwardRef,
  lazy,
  Suspense,
  useEffect,
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ClipboardEvent,
  type KeyboardEvent,
  type MutableRefObject,
} from 'react'
import { withAttachment, type MentionCandidate, type MentionPick } from '@shared/core'
import { sendFiles, sendMessage, useStore, type UiMessage } from '../store'
import { closedFor, fieldMentions, mentionTrigger, takeMention, type MentionTrigger } from '../format'
import { MentionBackdrop, MentionList, mentionRowId } from './Mentions'
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
/** The mentions picked for the text of a draft (docs/mentions.md „Entwurf“). */
const pickDrafts = new Map<string, MentionPick[]>()
const fileDrafts = new Map<string, Draft[]>()

const NO_PICKS: MentionPick[] = []
/** As many suggestions are asked for (docs/mentions.md „Abfrage“). */
const MENTION_ROWS = 50

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
  // The mentions picked for `text`; where they stand follows from the text alone.
  const [picks, setPicks] = useState<MentionPick[]>(() => pickDrafts.get(key) ?? NO_PICKS)
  const [files, setFiles] = useState<Draft[]>(() => fileDrafts.get(key) ?? [])
  const [staging, setStaging] = useState(0)
  const [error, setError] = useState('')
  const [emoji, setEmoji] = useState(false)
  const [saving, setSaving] = useState(false)
  /** The field at its large height (docs/chat.md „Eingabefeld vergrößern“); not kept. */
  const [large, setLarge] = useState(false)
  const wasLarge = useRef(false)
  const editingRef = useRef(editing)
  editingRef.current = editing
  const input = useRef<HTMLTextAreaElement | null>(null)
  const backdrop = useRef<HTMLDivElement>(null)
  // The suggestions after an „@“ (docs/mentions.md „Vorschlagsliste“).
  const [focused, setFocused] = useState(false)
  /** Where the caret stands; null with a selection. */
  const [caret, setCaret] = useState<number | null>(null)
  const [rows, setRows] = useState<MentionCandidate[]>([])
  const [chosen, setChosen] = useState(0)
  /** Esc or taking a row closed the list for this „@“ and this search text. */
  const [dismissed, setDismissed] = useState<MentionTrigger | null>(null)
  const asked = useRef(0)
  const listId = useId()
  const room = useStore((s) => s.rooms.find((r) => r.token === token))
  const picker = useRef<HTMLInputElement>(null)
  const emojiButton = useRef<HTMLButtonElement>(null)
  const recording = useVoice((s) => recordingFor(s, token))
  const voiceDraft = useVoice((s) => s.drafts[key] ?? null)
  const notice = useVoice((s) => (s.notice?.key === key ? s.notice.text : null))

  useEffect(() => {
    setText(textDrafts.get(key) ?? '')
    setPicks(pickDrafts.get(key) ?? NO_PICKS)
    setFiles(fileDrafts.get(key) ?? [])
    setError('')
    setLarge(false)
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
        if (recording) {
          setLarge(false)
          void sendRecording(replyTo).then(onSent)
        } else if (!editing && !voiceDraft && !text.trim() && files.length === 0) void startRecording(token)
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
      // Saved or cancelled: small again.
      setLarge(false)
      setText(textDrafts.get(key) ?? '')
      setPicks(pickDrafts.get(key) ?? NO_PICKS)
      input.current?.focus()
      return
    }
    void stopToDraft()
    setEmoji(false)
    setText(editing.source)
    setPicks(editing.mentions)
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

  const ranges = useMemo(() => fieldMentions(text, picks), [text, picks])
  const marked = ranges.length > 0

  /** The layer with the marks lies exactly behind the text: as wide, and scrolled alike. */
  function syncBackdrop() {
    const el = input.current
    const layer = backdrop.current
    if (!el || !layer) return
    // A scrollbar narrows the text.
    layer.style.width = `${el.clientWidth}px`
    layer.scrollTop = el.scrollTop
  }

  /** Scrolls the field so that the caret is in its visible part. */
  function revealCaret() {
    const el = input.current
    if (!el?.parentElement || el.scrollHeight <= el.clientHeight) return
    // The text up to the caret with the breaks of the field: where it ends, the caret stands.
    const probe = document.createElement('div')
    probe.className = 'composer-backdrop'
    probe.style.cssText = `bottom: auto; visibility: hidden; width: ${el.clientWidth}px`
    probe.textContent = el.value.slice(0, el.selectionEnd)
    const mark = probe.appendChild(document.createElement('span'))
    mark.textContent = '\u200b'
    el.parentElement.appendChild(probe)
    const top = mark.offsetTop
    const bottom = top + mark.offsetHeight
    probe.remove()
    const pad = parseFloat(getComputedStyle(el).paddingTop)
    if (top - pad < el.scrollTop) el.scrollTop = top - pad
    else if (bottom + pad > el.scrollTop + el.clientHeight) el.scrollTop = bottom + pad - el.clientHeight
  }

  // Auto-grow up to the CSS max-height, then scroll (like Signal). Large, the height is fixed
  // by the CSS.
  useLayoutEffect(() => {
    const el = input.current
    if (!el) return
    if (large) {
      el.style.height = ''
      el.style.overflowY = 'auto'
    } else {
      el.style.height = 'auto'
      const height = el.scrollHeight + el.offsetHeight - el.clientHeight
      el.style.height = `${height}px`
      el.style.overflowY = height > el.offsetHeight ? 'auto' : 'hidden'
    }
    // Another height shows another part of the text.
    if (wasLarge.current !== large) {
      wasLarge.current = large
      revealCaret()
    }
    syncBackdrop()
  }, [text, marked, large])
  // Another width breaks the lines anew.
  useEffect(() => {
    const el = input.current
    if (!el) return
    const observer = new ResizeObserver(syncBackdrop)
    observer.observe(el)
    return () => observer.disconnect()
  }, [])

  function updateText(v: string) {
    setText(v)
    // The edited text never becomes the draft.
    if (editing) return
    if (v) textDrafts.set(key, v)
    else textDrafts.delete(key)
  }

  function updatePicks(list: MentionPick[]) {
    setPicks(list)
    if (editing) return
    if (list.length) pickDrafts.set(key, list)
    else pickDrafts.delete(key)
  }

  function readCaret() {
    const el = input.current
    if (el) setCaret(el.selectionStart === el.selectionEnd ? el.selectionStart : null)
  }
  // The caret moves without the text changing (arrow keys, a click).
  useEffect(() => {
    const onSelection = () => document.activeElement === input.current && readCaret()
    document.addEventListener('selectionchange', onSelection)
    return () => document.removeEventListener('selectionchange', onSelection)
  }, [])

  // Also without the focus: Esc holds for its search text when the focus comes back.
  const trigger = useMemo(
    () => (caret != null && !saving ? mentionTrigger(text, caret, picks) : null),
    [caret, saving, text, picks],
  )
  const at = trigger?.at
  const query = trigger?.query
  // Asked on every change, without waiting: the core answers from memory. An answer that
  // was overtaken is dropped; the rows before stay until the next ones are there.
  useEffect(() => {
    const seq = ++asked.current
    if (query == null) {
      setRows([])
      setDismissed(null)
      return
    }
    // Closed (Esc, a row taken) holds until the search text changes.
    setDismissed((d) => (at != null && closedFor(d, { at, query }) ? d : null))
    window.talk.mentionCandidates(token, query, MENTION_ROWS).then(
      (list) => {
        if (seq !== asked.current) return
        setRows(list)
        setChosen(0)
      },
      () => seq === asked.current && setRows([]),
    )
  }, [token, at, query])
  const suggesting = focused && !!trigger && !dismissed && rows.length > 0
  const chosenRow = rows[Math.min(chosen, rows.length - 1)]

  /**
   * Replaces a part of the text through the editor, so undo knows about it; the caret
   * stands at `caretAt` afterwards.
   */
  function replaceRange(from: number, to: number, insert: string, caretAt: number) {
    const el = input.current
    if (!el) return
    el.focus()
    el.setSelectionRange(from, to)
    const done = insert ? document.execCommand('insertText', false, insert) : document.execCommand('delete')
    if (done) {
      el.setSelectionRange(caretAt, caretAt)
      readCaret()
      return
    }
    updateText(text.slice(0, from) + insert + text.slice(to))
    setCaret(caretAt)
    requestAnimationFrame(() => el.setSelectionRange(caretAt, caretAt))
  }

  /** Takes a row of the list: „@Name “ replaces what was typed from the „@“ to the caret. */
  function acceptMention(row: MentionCandidate) {
    if (!trigger || caret == null) return
    // An older pick with the same name gives way.
    updatePicks([...picks.filter((p) => p.label !== row.label), { label: row.label, mentionId: row.mentionId }])
    const taken = takeMention(text, trigger, caret, row.label)
    // Closed as after Esc: „Lena Brandt“ does not open the list again right behind „@Lena “.
    setDismissed(taken.closed)
    replaceRange(trigger.at, caret, taken.insert, taken.caret)
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
    const mentions = picks
    updateText('')
    updatePicks(NO_PICKS)
    updateFiles(() => [])
    setEmoji(false)
    setLarge(false)
    if (queued.length > 0) {
      void sendFiles(
        token,
        queued.map((d) => d.file),
        value,
        mentions,
        replyTo,
      ).finally(() => queued.forEach(releaseDraft))
    } else {
      void sendMessage(token, value, mentions, replyTo)
    }
    onSent()
  }

  // Off while the text is empty or the same as before (both without whitespace at the edges).
  const canSave = !!editing && !saving && text.trim() !== '' && text.trim() !== editing.source.trim()

  async function save() {
    if (!editing || !canSave) return
    setSaving(true)
    const error = await window.talk.editMessage(token, editing.msg.id, text.trim(), picks).catch(() => 'Nachricht nicht geändert')
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
    const bare = !e.shiftKey && !e.altKey && !e.ctrlKey && !e.metaKey
    // The keys of the suggestions come before every other rule of the field.
    if (suggesting && bare && !e.nativeEvent.isComposing) {
      if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
        e.preventDefault()
        const step = e.key === 'ArrowDown' ? 1 : rows.length - 1
        setChosen((i) => (Math.min(i, rows.length - 1) + step) % rows.length)
        return
      }
      if (e.key === 'Enter' || e.key === 'Tab') {
        e.preventDefault()
        acceptMention(chosenRow)
        return
      }
      if (e.key === 'Escape') {
        e.preventDefault()
        e.stopPropagation()
        setDismissed(trigger)
        return
      }
    }
    // Deleting backwards right behind a mention removes it as a whole.
    if (e.key === 'Backspace' && bare && !saving) {
      const el = e.currentTarget
      const hit = el.selectionStart === el.selectionEnd ? ranges.find(([, to]) => to === el.selectionStart) : undefined
      if (hit) {
        e.preventDefault()
        replaceRange(hit[0], hit[1], '', hit[0])
        return
      }
    }
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

  // Without a text field (a recording, a voice draft) there is nothing to enlarge.
  const voiceBar = !editing && (recording || voiceDraft)
  const resizeLabel = large ? 'Eingabefeld verkleinern' : 'Eingabefeld vergrößern'

  return (
    <div className="composer">
      {!voiceBar && (
        <button
          className="composer-resize"
          aria-label={resizeLabel}
          title={resizeLabel}
          aria-pressed={large}
          // The focus and the caret stay in the field.
          onMouseDown={(e) => e.preventDefault()}
          onClick={() => setLarge((v) => !v)}
        >
          {/* A flat arrow, 12 wide and 4 high: up, and down while the field is large. */}
          <svg viewBox="0 0 24 24" width="24" height="24" aria-hidden="true">
            <path
              d={large ? 'M6 10l6 4 6-4' : 'M6 14l6-4 6 4'}
              fill="none"
              stroke="currentColor"
              strokeWidth="1.6"
              strokeLinecap="round"
              strokeLinejoin="round"
            />
          </svg>
        </button>
      )}
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
            <div className="quote-text">{editing.msg.text.trim() || editing.msg.attachment?.contactName || editing.msg.attachment?.name || ''}</div>
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
            setLarge(false)
            void sendRecording(replyTo).then(onSent)
            onCancelReply()
          }}
        />
      ) : voiceDraft && !editing ? (
        <DraftBar
          draft={voiceDraft}
          onDiscard={() => void discardDraft(token)}
          onSend={() => {
            setLarge(false)
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
        <div className="composer-field">
          {suggesting && (
            <MentionList
              id={listId}
              rows={rows}
              chosen={Math.min(chosen, rows.length - 1)}
              token={token}
              roomName={room?.name ?? ''}
              avatarVersion={room?.avatarVersion ?? ''}
              onChoose={setChosen}
              onAccept={acceptMention}
            />
          )}
          {marked && (
            <div className="composer-backdrop" ref={backdrop} aria-hidden="true">
              <MentionBackdrop text={text} ranges={ranges} />
            </div>
          )}
          <textarea
            ref={(el) => {
              input.current = el
              if (typeof ref === 'function') ref(el)
              else if (ref) ref.current = el
            }}
            className={`composer-input${large ? ' large' : ''}`}
            rows={1}
            placeholder={files.length && !editing ? 'Bildunterschrift hinzufügen' : 'Nachricht'}
            value={text}
            readOnly={saving}
            aria-controls={suggesting ? listId : undefined}
            aria-activedescendant={suggesting ? mentionRowId(listId, Math.min(chosen, rows.length - 1)) : undefined}
            onChange={(e) => {
              updateText(e.target.value)
              readCaret()
            }}
            onKeyDown={onKeyDown}
            onPaste={onPaste}
            onScroll={syncBackdrop}
            onFocus={() => {
              setFocused(true)
              readCaret()
            }}
            onBlur={() => setFocused(false)}
          />
        </div>
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
