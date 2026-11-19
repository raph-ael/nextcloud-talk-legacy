import type { GeoLocation, MentionPick, MentionView } from '@shared/core'

const time = new Intl.DateTimeFormat('de-DE', { hour: '2-digit', minute: '2-digit' })
const weekday = new Intl.DateTimeFormat('de-DE', { weekday: 'short' })
const shortDate = new Intl.DateTimeFormat('de-DE', { day: '2-digit', month: '2-digit', year: '2-digit' })
const dayInHalfYear = new Intl.DateTimeFormat('de-DE', { weekday: 'short', day: 'numeric', month: 'short' })
const dayWithYear = new Intl.DateTimeFormat('de-DE', { day: 'numeric', month: 'short', year: 'numeric' })
const full = new Intl.DateTimeFormat('de-DE', { dateStyle: 'full', timeStyle: 'short' })

function startOfDay(d: Date): number {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime()
}

function daysAgo(unix: number): number {
  return Math.round((startOfDay(new Date()) - startOfDay(new Date(unix * 1000))) / 86_400_000)
}

const monthDay = new Intl.DateTimeFormat('de-DE', { day: 'numeric', month: 'short' })

/** Conversation list, like Signal: Jetzt · 5 Min. · 3 Std. · Fr. · 12. Okt. · 12.10.25 */
export function listTime(unix: number, nowMs: number = Date.now()): string {
  if (!unix) return ''
  const secs = Math.floor(nowMs / 1000) - unix
  const date = new Date(unix * 1000)
  if (secs < 60) return 'Jetzt'
  if (secs < 3600) return `${Math.floor(secs / 60)} Min.`
  if (secs < 24 * 3600) return `${Math.floor(secs / 3600)} Std.`
  if (daysAgo(unix) < 7) return weekday.format(date)
  if (date.getFullYear() === new Date(nowMs).getFullYear()) return monthDay.format(date)
  return shortDate.format(date)
}

/** 1,2 MB style file size. */
export function fileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${Math.floor(bytes / 1024)} KB`
  return `${(bytes / (1024 * 1024)).toLocaleString('de-DE', { minimumFractionDigits: 1, maximumFractionDigits: 1 })} MB`
}

/** „52,5283° N, 13,4356° O“ */
export function coordinates(l: GeoLocation): string {
  const part = (v: number, positive: string, negative: string) =>
    `${Math.abs(v).toLocaleString('de-DE', { minimumFractionDigits: 4, maximumFractionDigits: 4 })}° ${v < 0 ? negative : positive}`
  return `${part(l.latitude, 'N', 'S')}, ${part(l.longitude, 'O', 'W')}`
}

/** The place on openstreetmap.org, with a marker (docs/attachments.md). */
export function locationLink(l: GeoLocation): string {
  // Always six decimals, as in the core's `geo:` and in Talk's web client.
  const lat = l.latitude.toFixed(6)
  const lon = l.longitude.toFixed(6)
  return `https://www.openstreetmap.org/?mlat=${lat}&mlon=${lon}#map=16/${lat}/${lon}`
}

export function messageTime(unix: number): string {
  return time.format(new Date(unix * 1000))
}

export function fullTime(unix: number): string {
  return full.format(new Date(unix * 1000))
}

/**
 * Day separators and the floating date (docs/chat.md): Heute · Gestern · Mi., 11. Nov.
 * (up to six months back) · 11. Nov. 2025
 */
export function dayLabel(unix: number, nowMs: number = Date.now()): string {
  const date = new Date(unix * 1000)
  const now = new Date(nowMs)
  const d = Math.round((startOfDay(now) - startOfDay(date)) / 86_400_000)
  if (d === 0) return 'Heute'
  if (d === 1) return 'Gestern'
  const halfYear = new Date(now.getFullYear(), now.getMonth() - 6, now.getDate())
  return date >= halfYear ? dayInHalfYear.format(date) : dayWithYear.format(date)
}

export function sameDay(a: number, b: number): boolean {
  return startOfDay(new Date(a * 1000)) === startOfDay(new Date(b * 1000))
}

const PALETTE = ['#d0463b', '#d9822b', '#3d9a50', '#2c7bd6', '#6b5bd2', '#c2408f', '#2a9a9e', '#8a7350']

/** Stable color per name (same hash as the mobile apps). */
export function colorFor(name: string): string {
  let h = 0
  for (const ch of name) h = (Math.imul(h, 31) + ch.codePointAt(0)!) & 0x7fffffff
  return PALETTE[h % PALETTE.length]
}

export function initials(name: string): string {
  const parts = name.split(/[\s.]+/).filter(Boolean).slice(0, 2)
  const letters = parts.map((p) => [...p][0]).join('')
  return letters ? letters.toUpperCase() : '?'
}

const EMOJI_ONLY = /^(?:\p{Extended_Pictographic}|\p{Emoji_Component}|‍|️|\s)+$/u

/** 1–3 emoji without text are shown large, like in Signal. */
export function isJumboEmoji(text: string): boolean {
  if (!EMOJI_ONLY.test(text) || !/\p{Extended_Pictographic}/u.test(text)) return false
  const count = [...new Intl.Segmenter().segment(text.trim())].filter((s) => s.segment.trim()).length
  return count <= 3
}

const URL_RE = /\bhttps?:\/\/[^\s<>"']+[^\s<>"'.,;:!?)\]]/g

export function splitLinks(text: string): Array<{ text: string; href?: string }> {
  const out: Array<{ text: string; href?: string }> = []
  let last = 0
  for (const m of text.matchAll(URL_RE)) {
    if (m.index > last) out.push({ text: text.slice(last, m.index) })
    out.push({ text: m[0], href: m[0] })
    last = m.index + m[0].length
  }
  if (last < text.length) out.push({ text: text.slice(last) })
  return out
}

// --- search words in a bubble (docs/search.md „Hervorhebung“) -------------------------------

/** What the search compares, as in the core: lowercase and without accents; „ß“ stays „ß“. */
export function fold(text: string): string {
  return text.toLowerCase().normalize('NFD').replace(/\p{M}/gu, '')
}

/** The folded words of a search text, or null below two characters. */
export function searchWords(query: string): string[] | null {
  const words = [...new Set(query.split(/\s+/).map(fold).filter(Boolean))]
  return words.reduce((n, w) => n + [...w].length, 0) >= 2 ? words : null
}

/**
 * Splits `text` at the search words: without case and accents, also inside a word. Parts
 * with `hit` get the mark; the text itself stays as it is.
 */
export function splitMarks(text: string, words: string[] | null | undefined): Array<{ text: string; hit: boolean }> {
  if (!words?.length || !text) return [{ text, hit: false }]
  // Folding may drop or add characters: `at[i]` is where folded character i starts in `text`.
  let folded = ''
  const at: number[] = []
  for (let i = 0; i < text.length; ) {
    const char = String.fromCodePoint(text.codePointAt(i)!)
    const f = fold(char)
    for (let k = 0; k < f.length; k++) at.push(i)
    folded += f
    i += char.length
  }
  at.push(text.length)
  const ranges: Array<[number, number]> = []
  for (const w of words)
    for (let i = folded.indexOf(w); i >= 0; i = folded.indexOf(w, i + 1)) ranges.push([at[i], at[i + w.length]])
  if (ranges.length === 0) return [{ text, hit: false }]
  ranges.sort((a, b) => a[0] - b[0])
  const out: Array<{ text: string; hit: boolean }> = []
  let pos = 0
  let [from, to] = ranges[0]
  const flush = () => {
    if (from > pos) out.push({ text: text.slice(pos, from), hit: false })
    if (to > from) out.push({ text: text.slice(from, to), hit: true })
    pos = Math.max(pos, to)
  }
  for (const [a, b] of ranges.slice(1)) {
    if (a <= to) to = Math.max(to, b)
    else {
      flush()
      from = a
      to = b
    }
  }
  flush()
  if (pos < text.length) out.push({ text: text.slice(pos), hit: false })
  return out
}

// --- mentions (docs/mentions.md) -------------------------------------------------------------

/** A letter or a digit, as the core counts them: a name does not end before one. */
const WORD_CHAR = /[\p{Alphabetic}\p{N}]/u
/** A mention stands at the start of the text or after one of these. */
const BEFORE_MENTION = /[\s\-.'@]/
/** The server reads a mention only with these characters in its id (as in the core). */
const WIRE_ID = /^[A-Za-z0-9_\-@.' /:]+$/

function endsName(text: string, at: number): boolean {
  const next = text.codePointAt(at)
  return next == null || !WORD_CHAR.test(String.fromCodePoint(next))
}

const byLength = (a: string, b: string) => [...b].length - [...a].length

/**
 * Where the picks stand as mentions in the text of the input field, as [from, to) with the
 * „@“. The rule of the core (docs/mentions.md „Im Eingabefeld“): `@{label}` after the start,
 * white space or one of `-.'@`, and no letter or digit after it; the longest name counts.
 */
export function fieldMentions(text: string, picks: MentionPick[]): Array<[number, number]> {
  if (picks.length === 0 || !text.includes('@')) return []
  const labels = [...new Set(picks.filter((p) => p.label && WIRE_ID.test(p.mentionId)).map((p) => p.label))].sort(byLength)
  const out: Array<[number, number]> = []
  let end = -1
  for (let i = text.indexOf('@'); i >= 0; i = text.indexOf('@', i + 1)) {
    // Right behind a mention none starts (the server sees its closing quote there).
    if (i < end || i === end || (i > 0 && !BEFORE_MENTION.test(text[i - 1]))) continue
    const label = labels.find((l) => text.startsWith(l, i + 1) && endsName(text, i + 1 + l.length))
    if (label == null) continue
    end = i + 1 + label.length
    out.push([i, end])
  }
  return out
}

/** The text being typed after an „@“: the place of the „@“ and what follows up to the caret. */
export interface MentionTrigger {
  at: number
  query: string
}

/** Longest search text after the „@“. */
const MENTION_QUERY_MAX = 40

/**
 * The suggestions' search text (docs/mentions.md „Vorschlagsliste“): from the nearest „@“
 * before the caret that stands at the start or after white space. None across a line break,
 * beyond 40 characters, or when a finished mention starts at that „@“.
 */
export function mentionTrigger(text: string, caret: number, picks: MentionPick[]): MentionTrigger | null {
  for (let i = caret - 1; i >= 0; i--) {
    const ch = text[i]
    if (ch === '\n') return null
    if (ch !== '@' || (i > 0 && !/\s/.test(text[i - 1]))) continue
    const query = text.slice(i + 1, caret)
    if ([...query].length > MENTION_QUERY_MAX) return null
    return fieldMentions(text, picks).some(([from]) => from === i) ? null : { at: i, query }
  }
  return null
}

/**
 * Splits the text of a bubble at its mentions (docs/mentions.md „In der Nachricht“): every
 * `label` („@Lena Brandt“) that no letter or digit follows, the longest name first.
 */
export function splitMentions(text: string, mentions: MentionView[]): Array<{ text: string; mention?: MentionView }> {
  if (mentions.length === 0 || !text.includes('@')) return [{ text }]
  const known = [...mentions].filter((m) => m.label.length > 1).sort((a, b) => byLength(a.label, b.label))
  const out: Array<{ text: string; mention?: MentionView }> = []
  let pos = 0
  for (let i = text.indexOf('@'); i >= 0; i = text.indexOf('@', i + 1)) {
    if (i < pos) continue
    const m = known.find((k) => text.startsWith(k.label, i) && endsName(text, i + k.label.length))
    if (!m) continue
    if (i > pos) out.push({ text: text.slice(pos, i) })
    pos = i + m.label.length
    out.push({ text: text.slice(i, pos), mention: m })
  }
  if (pos < text.length) out.push({ text: text.slice(pos) })
  return out
}
