const time = new Intl.DateTimeFormat('de-DE', { hour: '2-digit', minute: '2-digit' })
const weekday = new Intl.DateTimeFormat('de-DE', { weekday: 'short' })
const shortDate = new Intl.DateTimeFormat('de-DE', { day: '2-digit', month: '2-digit', year: '2-digit' })
const longDay = new Intl.DateTimeFormat('de-DE', { weekday: 'long', day: 'numeric', month: 'long' })
const longDayYear = new Intl.DateTimeFormat('de-DE', { weekday: 'long', day: 'numeric', month: 'long', year: 'numeric' })
const full = new Intl.DateTimeFormat('de-DE', { dateStyle: 'full', timeStyle: 'short' })

function startOfDay(d: Date): number {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime()
}

function daysAgo(unix: number): number {
  return Math.round((startOfDay(new Date()) - startOfDay(new Date(unix * 1000))) / 86_400_000)
}

/** Conversation list: 14:03 · Gestern · Mo. · 03.11.26 */
export function listTime(unix: number): string {
  if (!unix) return ''
  const d = daysAgo(unix)
  const date = new Date(unix * 1000)
  if (d === 0) return time.format(date)
  if (d === 1) return 'Gestern'
  if (d < 7) return weekday.format(date)
  return shortDate.format(date)
}

export function messageTime(unix: number): string {
  return time.format(new Date(unix * 1000))
}

export function fullTime(unix: number): string {
  return full.format(new Date(unix * 1000))
}

/** Day separators in the timeline. */
export function dayLabel(unix: number): string {
  const d = daysAgo(unix)
  const date = new Date(unix * 1000)
  if (d === 0) return 'Heute'
  if (d === 1) return 'Gestern'
  return date.getFullYear() === new Date().getFullYear() ? longDay.format(date) : longDayYear.format(date)
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
