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
