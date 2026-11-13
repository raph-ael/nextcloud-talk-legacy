import { lazy, Suspense, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import type { ReactionView } from '@shared/core'
import { useStore, type UiMessage } from '../store'
import { toast } from '../call/store'
import { Avatar } from './Avatar'

const EmojiPicker = lazy(() => import('./EmojiPicker').then((m) => ({ default: m.EmojiPicker })))

/** Signal's six (docs/chat.md „Reaktionen“). */
const QUICK = ['❤️', '👍', '👎', '😂', '😮', '😢']

/** May the user react to this message? Delivered text, file and voice messages only. */
export function reactable(canReact: boolean, m: UiMessage): boolean {
  return canReact && m.kind === 'text' && !m.status
}

/** Sets, replaces or (same emoji again / `null`) removes the own reaction; errors go to a toast. */
export function react(token: string, messageId: number, emoji: string | null): Promise<void> {
  return window.talk
    .setReaction(token, messageId, emoji)
    .then((err) => {
      if (err) toast(err)
    })
    .catch(() => toast('Reaktion nicht gesendet'))
}

/** An own reaction outside the six takes the last place (only for this display). */
function quickFor(own: string[]): string[] {
  const extra = own.find((e) => !QUICK.includes(e))
  return extra ? [...QUICK.slice(0, 5), extra] : QUICK
}

/** The quick bar: six emojis and „＋“. */
export function ReactionBar({ msg, onPick, onMore }: { msg: UiMessage; onPick: (emoji: string) => void; onMore: () => void }) {
  return (
    <div className="reaction-bar" role="group" aria-label="Reagieren">
      {quickFor(msg.reactionsSelf).map((e) => (
        <button
          key={e}
          className={`reaction-bar-emoji${msg.reactionsSelf.includes(e) ? ' on' : ''}`}
          aria-label={`Mit ${e} reagieren`}
          aria-pressed={msg.reactionsSelf.includes(e)}
          onClick={() => onPick(e)}
        >
          {e}
        </button>
      ))}
      <button className="reaction-bar-more" aria-label="Weitere Reaktionen" title="Weitere Reaktionen" onClick={onMore}>
        <svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true">
          <path fill="currentColor" d="M11 5h2v6h6v2h-6v6h-2v-6H5v-2h6z" />
        </svg>
      </button>
    </div>
  )
}

/**
 * The bar as a popover above the bubble (hover action „Reagieren“); „＋“ turns it into the
 * emoji picker in place. Closes on pick, Esc, outside click and window resize.
 */
export function ReactionPopover({
  token,
  msg,
  anchor,
  toggle,
  startFull = false,
  onClose,
}: {
  token: string
  msg: UiMessage
  /** The bubble's rectangle. */
  anchor: DOMRect
  /** The button that opened it: clicks there toggle instead of counting as outside. */
  toggle: HTMLElement | null
  /** Open straight in the emoji picker („＋“ in the context menu). */
  startFull?: boolean
  onClose: () => void
}) {
  const ref = useRef<HTMLDivElement>(null)
  const [full, setFull] = useState(startFull)
  const [pos, setPos] = useState<{ left: number; top: number } | null>(null)

  // Above the bubble, else below it, always inside the window; again when the size changes
  // (the lazily loaded picker replaces its placeholder).
  useLayoutEffect(() => {
    const el = ref.current
    if (!el) return
    const place = () => {
      const r = el.getBoundingClientRect()
      let left = msg.isOwn ? anchor.right - r.width : anchor.left
      left = Math.max(8, Math.min(left, window.innerWidth - r.width - 8))
      let top = anchor.top - r.height - 6
      if (top < 8) top = Math.min(anchor.bottom + 6, window.innerHeight - r.height - 8)
      setPos({ left, top: Math.max(8, top) })
    }
    place()
    if (!full) el.querySelector<HTMLButtonElement>('button')?.focus()
    const ro = new ResizeObserver(place)
    ro.observe(el)
    return () => ro.disconnect()
  }, [anchor, full, msg.isOwn])

  useEffect(() => {
    function onDown(e: MouseEvent) {
      const t = e.target as Node
      if (full) return // the picker reports outside clicks itself
      if (!ref.current?.contains(t) && !toggle?.contains(t)) onClose()
    }
    function onKey(e: KeyboardEvent) {
      if (e.key !== 'Escape') return
      e.preventDefault()
      e.stopPropagation()
      onClose()
    }
    window.addEventListener('mousedown', onDown, true)
    window.addEventListener('keydown', onKey, true)
    window.addEventListener('blur', onClose)
    window.addEventListener('resize', onClose)
    return () => {
      window.removeEventListener('mousedown', onDown, true)
      window.removeEventListener('keydown', onKey, true)
      window.removeEventListener('blur', onClose)
      window.removeEventListener('resize', onClose)
    }
  }, [full, toggle, onClose])

  const pickerAnchor = useMemo(() => ({ current: toggle }), [toggle])
  const pick = (emoji: string) => {
    onClose()
    void react(token, msg.id, emoji)
  }

  return (
    <div
      ref={ref}
      className={`reaction-popover${full ? ' full' : ''}`}
      style={pos ?? { left: -9999, top: -9999 }}
    >
      {full ? (
        <Suspense fallback={<div className="reaction-picker-loading" />}>
          <EmojiPicker anchor={pickerAnchor} onPick={pick} onClose={onClose} />
        </Suspense>
      ) : (
        <ReactionBar msg={msg} onPick={pick} onMore={() => setFull(true)} />
      )}
    </div>
  )
}

// --- who reacted ---

/** Names per message and reaction state, fetched once on hover (docs/chat.md). */
const cache = new Map<string, Promise<ReactionView[]>>()

function stateKey(token: string, m: UiMessage): string {
  return `${token}\n${m.id}\n${JSON.stringify(m.reactions)}\n${m.reactionsSelf.join(',')}`
}

function fetchReactions(token: string, m: UiMessage): Promise<ReactionView[]> {
  const key = stateKey(token, m)
  let p = cache.get(key)
  if (!p) {
    p = window.talk.reactions(token, m.id).catch(() => [])
    cache.set(key, p)
    if (cache.size > 200) cache.delete(cache.keys().next().value!)
  }
  return p
}

function displayName(r: ReactionView): string {
  return r.isOwn ? 'Du' : r.name || r.actorId
}

/** The pills under the bubble: hover shows the names, click opens „Wer hat reagiert“. */
export function ReactionPills({ token, msg, onOpen }: { token: string; msg: UiMessage; onOpen: (m: UiMessage) => void }) {
  const entries = Object.entries(msg.reactions)
  const keys = entries.map(([e]) => pillKey(e, msg))
  // Pills present when the row appeared don't pop; later ones (and the own one) do.
  const initial = useRef<Set<string> | null>(null)
  if (initial.current === null) initial.current = new Set(keys)
  const [tip, setTip] = useState<{ emoji: string; names: string | null } | null>(null)
  const hovered = useRef<string | null>(null)

  async function showTip(emoji: string) {
    hovered.current = emoji
    setTip({ emoji, names: null })
    const list = await fetchReactions(token, msg)
    if (hovered.current !== emoji) return
    const names = list.filter((r) => r.emoji === emoji).map(displayName)
    setTip({ emoji, names: names.length > 12 ? `${names.slice(0, 12).join(', ')} und ${names.length - 12} weitere` : names.join(', ') })
  }

  // The reaction state changed under the pointer: refresh the names.
  const sk = stateKey(token, msg)
  useEffect(() => {
    if (hovered.current && msg.reactions[hovered.current]) void showTip(hovered.current)
    else if (hovered.current) {
      hovered.current = null
      setTip(null)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sk])

  // Always mounted (also without reactions) so the first pill pops too.
  if (entries.length === 0) return null
  return (
    <div className="reactions">
      {entries.map(([emoji, count], i) => {
        const mine = msg.reactionsSelf.includes(emoji)
        return (
          <button
            key={keys[i]}
            className={`reaction-pill${mine ? ' mine' : ''}${initial.current!.has(keys[i]) ? '' : ' pop'}`}
            aria-label={`${emoji} ${count}, Reaktionen anzeigen`}
            onClick={() => {
              hovered.current = null
              setTip(null)
              onOpen(msg)
            }}
            onMouseEnter={() => void showTip(emoji)}
            onMouseLeave={() => {
              hovered.current = null
              setTip(null)
            }}
            onFocus={() => void showTip(emoji)}
            onBlur={() => {
              hovered.current = null
              setTip(null)
            }}
          >
            {emoji}
            {count > 1 && <small>{count}</small>}
            {tip?.emoji === emoji && (
              <span className="reaction-tip" role="tooltip">
                {tip.names ?? '…'}
              </span>
            )}
          </button>
        )
      })}
    </div>
  )
}

/** Own and others' pills are different elements, so becoming „mine“ pops as well. */
function pillKey(emoji: string, m: UiMessage): string {
  return m.reactionsSelf.includes(emoji) ? `${emoji}\u0000own` : emoji
}

/** Small modal „Wer hat reagiert“: tabs „Alle · N“ and per emoji, own row removes on click. */
export function WhoReacted({ token, messageId, onClose }: { token: string; messageId: number; onClose: () => void }) {
  const msg = useStore((s) => s.chats[token]?.messages.find((m) => m.id === messageId))
  const [list, setList] = useState<ReactionView[] | null>(null)
  const [tab, setTab] = useState<string | null>(null)
  /** Bumped after an own removal reached the server, to load the list again. */
  const [reload, setReload] = useState(0)
  const ref = useRef<HTMLDivElement>(null)
  const sk = msg ? stateKey(token, msg) : ''

  // Fresh on open, and again whenever the message's reactions change (live updates) or an own
  // removal is done (a fetch during the removal may still see the old state).
  useEffect(() => {
    let live = true
    window.talk
      .reactions(token, messageId)
      .then((l) => live && setList(l))
      .catch(() => live && setList([]))
    return () => {
      live = false
    }
  }, [token, messageId, sk, reload])

  function removeOwn() {
    setList((l) => l && l.filter((r) => !r.isOwn))
    void react(token, messageId, null).then(() => setReload((n) => n + 1))
  }

  const tabs = useMemo(() => {
    const counts = new Map<string, number>()
    for (const r of list ?? []) counts.set(r.emoji, (counts.get(r.emoji) ?? 0) + 1)
    return [...counts.entries()].sort((a, b) => b[1] - a[1])
  }, [list])

  // Nothing left (or the message is gone): close.
  useEffect(() => {
    if (!msg || Object.keys(msg.reactions).length === 0) onClose()
  }, [msg, onClose])
  // The selected emoji disappeared: back to „Alle“.
  useEffect(() => {
    if (tab && list && !tabs.some(([e]) => e === tab)) setTab(null)
  }, [tab, tabs, list])

  useEffect(() => {
    ref.current?.focus()
    function onKey(e: KeyboardEvent) {
      if (e.key !== 'Escape') return
      e.preventDefault()
      e.stopPropagation()
      onClose()
    }
    window.addEventListener('keydown', onKey, true)
    return () => window.removeEventListener('keydown', onKey, true)
  }, [onClose])

  const rows = (list ?? []).filter((r) => !tab || r.emoji === tab)

  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="modal who-reacted" role="dialog" aria-modal="true" aria-label="Reaktionen" tabIndex={-1} ref={ref}>
        <div className="who-tabs" role="tablist">
          <button role="tab" aria-selected={tab === null} className={`who-tab${tab === null ? ' on' : ''}`} onClick={() => setTab(null)}>
            Alle · {list?.length ?? '…'}
          </button>
          {tabs.map(([emoji, n]) => (
            <button
              key={emoji}
              role="tab"
              aria-selected={tab === emoji}
              className={`who-tab${tab === emoji ? ' on' : ''}`}
              onClick={() => setTab(emoji)}
            >
              {emoji} {n}
            </button>
          ))}
        </div>
        <div className="who-list">
          {list === null
            ? [0, 1].map((i) => (
                <div key={i} className="who-row placeholder" aria-hidden>
                  <span className="who-avatar-ph" />
                  <span className="who-name-ph" />
                </div>
              ))
            : rows.map((r) => {
                const body = (
                  <>
                    <Avatar name={displayName(r)} userId={r.actorType === 'users' ? r.actorId : undefined} size={32} />
                    <span className="who-name">
                      <span className="ellipsis">{displayName(r)}</span>
                      {r.isOwn && <small>Tippen zum Entfernen</small>}
                    </span>
                    <span className="who-emoji">{r.emoji}</span>
                  </>
                )
                const key = `${r.actorType}/${r.actorId}/${r.emoji}`
                return r.isOwn ? (
                  <button key={key} className="who-row own" onClick={removeOwn}>
                    {body}
                  </button>
                ) : (
                  <div key={key} className="who-row">
                    {body}
                  </div>
                )
              })}
        </div>
      </div>
    </div>
  )
}
