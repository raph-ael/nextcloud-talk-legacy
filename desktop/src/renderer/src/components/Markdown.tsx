// Minimal Markdown renderer for chat messages (the subset Talk's own clients render):
// headings, lists, quotes, code blocks, **bold**, *italic*, ~~strike~~, `code`, links.
// Produces React elements only – no HTML injection possible.

import { Fragment, type ReactNode } from 'react'
import { splitMarks, splitMentions } from '../format'
import { Mention, type BubbleMentions } from './Mentions'

/** Search words of the chat's search, folded (docs/search.md „Hervorhebung“). */
export type Marks = string[] | null | undefined
/** The mentions of the message (docs/mentions.md „In der Nachricht“). */
export type Mentions = BubbleMentions | null | undefined

/**
 * `text` with the search words marked and, with `mentions`, the mentions drawn. A search
 * word on a mention lies on top of it.
 */
export function marked(text: string, marks: Marks, key: string | number = 0, mentions?: Mentions): ReactNode {
  if (mentions?.list.length && text.includes('@')) {
    const parts = splitMentions(text, mentions.list)
    if (parts.some((p) => p.mention))
      return parts.map((p, i) =>
        p.mention ? (
          <Mention key={`${key}/@${i}`} mention={p.mention} ctx={mentions}>
            {searchMarked(p.text, marks, `${key}/@${i}`)}
          </Mention>
        ) : (
          <Fragment key={`${key}/@${i}`}>{searchMarked(p.text, marks, `${key}/@${i}`)}</Fragment>
        ),
      )
  }
  return searchMarked(text, marks, key)
}

function searchMarked(text: string, marks: Marks, key: string | number): ReactNode {
  if (!marks?.length) return text
  const parts = splitMarks(text, marks)
  if (parts.length === 1 && !parts[0].hit) return text
  return parts.map((p, i) =>
    p.hit ? (
      <mark key={`${key}/${i}`} className="search-mark">
        {p.text}
      </mark>
    ) : (
      p.text
    ),
  )
}

const INLINE =
  /(`[^`\n]+`)|(\*\*[^*\n]+\*\*|__[^_\n]+__)|(~~[^~\n]+~~)|(\*[^*\s][^*\n]*\*|_[^_\s][^_\n]*_)|(\[[^\]\n]+\]\(https?:\/\/[^\s)]+\))|(https?:\/\/[^\s<>"']+[^\s<>"'.,;:!?)\]])/g

// Mentions are drawn in the text and inside its formats, not in code and not in a link.
function inline(text: string, key = 0, marks?: Marks, mentions?: Mentions): ReactNode[] {
  const out: ReactNode[] = []
  let last = 0
  const plainText = (t: string, at: number) => {
    const node = marked(t, marks, `${key}-t${at}`, mentions)
    if (Array.isArray(node)) out.push(...node)
    else out.push(node)
  }
  for (const m of text.matchAll(INLINE)) {
    if (m.index > last) plainText(text.slice(last, m.index), last)
    const [tok] = m
    const k = `${key}-${m.index}`
    if (m[1]) out.push(<code key={k}>{marked(tok.slice(1, -1), marks)}</code>)
    else if (m[2]) out.push(<strong key={k}>{inline(tok.slice(2, -2), m.index, marks, mentions)}</strong>)
    else if (m[3]) out.push(<s key={k}>{inline(tok.slice(2, -2), m.index, marks, mentions)}</s>)
    else if (m[4]) out.push(<em key={k}>{inline(tok.slice(1, -1), m.index, marks, mentions)}</em>)
    else if (m[5]) {
      const close = tok.indexOf('](')
      out.push(
        <a key={k} href={tok.slice(close + 2, -1)} target="_blank" rel="noreferrer">
          {marked(tok.slice(1, close), marks)}
        </a>,
      )
    } else if (m[6])
      out.push(
        <a key={k} href={tok} target="_blank" rel="noreferrer">
          {marked(tok, marks)}
        </a>,
      )
    last = m.index + tok.length
  }
  if (last < text.length) plainText(text.slice(last), last)
  return out
}

type Block =
  | { kind: 'p'; lines: string[] }
  | { kind: 'h'; text: string }
  | { kind: 'ul' | 'ol'; items: string[] }
  | { kind: 'quote'; lines: string[] }
  | { kind: 'code'; text: string }
  | { kind: 'hr' }

function parse(src: string): Block[] {
  const blocks: Block[] = []
  const lines = src.replace(/\r\n/g, '\n').split('\n')
  let i = 0
  const lastBlock = () => blocks[blocks.length - 1]
  while (i < lines.length) {
    const line = lines[i]
    if (line.trimStart().startsWith('```')) {
      const body: string[] = []
      i++
      while (i < lines.length && !lines[i].trimStart().startsWith('```')) body.push(lines[i++])
      i++
      blocks.push({ kind: 'code', text: body.join('\n') })
      continue
    }
    let m: RegExpMatchArray | null
    if ((m = line.match(/^\s{0,3}#{1,6}\s+(.*)$/))) blocks.push({ kind: 'h', text: m[1] })
    else if (/^\s{0,3}([-*_])(\s*\1){2,}\s*$/.test(line)) blocks.push({ kind: 'hr' })
    else if ((m = line.match(/^\s*[-*+]\s+(.*)$/))) {
      const b = lastBlock()
      if (b?.kind === 'ul') b.items.push(m[1])
      else blocks.push({ kind: 'ul', items: [m[1]] })
    } else if ((m = line.match(/^\s*\d+[.)]\s+(.*)$/))) {
      const b = lastBlock()
      if (b?.kind === 'ol') b.items.push(m[1])
      else blocks.push({ kind: 'ol', items: [m[1]] })
    } else if ((m = line.match(/^\s*>\s?(.*)$/))) {
      const b = lastBlock()
      if (b?.kind === 'quote') b.lines.push(m[1])
      else blocks.push({ kind: 'quote', lines: [m[1]] })
    } else if (line.trim() === '') {
      blocks.push({ kind: 'p', lines: [] })
    } else {
      const b = lastBlock()
      if (b?.kind === 'p' && b.lines.length > 0) b.lines.push(line)
      else blocks.push({ kind: 'p', lines: [line] })
    }
    i++
  }
  return blocks.filter((b) => b.kind !== 'p' || b.lines.length > 0)
}

const lines = (ls: string[], marks?: Marks, mentions?: Mentions) =>
  ls.map((l, i) => (
    <Fragment key={i}>
      {i > 0 && <br />}
      {inline(l, i, marks, mentions)}
    </Fragment>
  ))

export function Markdown({ text, marks, mentions }: { text: string; marks?: Marks; mentions?: Mentions }) {
  const blocks = parse(text)
  // Plain single-paragraph messages stay inline so the timestamp can float beside them.
  if (blocks.length === 1 && blocks[0].kind === 'p') return <>{lines(blocks[0].lines, marks, mentions)}</>
  return (
    <div className="md">
      {blocks.map((b, i) => {
        switch (b.kind) {
          case 'p':
            return <p key={i}>{lines(b.lines, marks, mentions)}</p>
          case 'h':
            return (
              <p key={i} className="md-h">
                {inline(b.text, 0, marks, mentions)}
              </p>
            )
          case 'ul':
          case 'ol': {
            const List = b.kind
            return (
              <List key={i}>
                {b.items.map((it, j) => (
                  <li key={j}>{inline(it, j, marks, mentions)}</li>
                ))}
              </List>
            )
          }
          case 'quote':
            return <blockquote key={i}>{lines(b.lines, marks, mentions)}</blockquote>
          case 'code':
            return <pre key={i}>{marked(b.text, marks)}</pre>
          case 'hr':
            return <hr key={i} />
        }
      })}
    </div>
  )
}

/** Markdown → plain one-liner (header descriptions). */
export function plain(text: string): string {
  return text
    .replace(/[*_~`#>]/g, '')
    .replace(/\s+/g, ' ')
    .trim()
}
