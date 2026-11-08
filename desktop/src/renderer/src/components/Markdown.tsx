// Minimal Markdown renderer for chat messages (the subset Talk's own clients render):
// headings, lists, quotes, code blocks, **bold**, *italic*, ~~strike~~, `code`, links.
// Produces React elements only – no HTML injection possible.

import { Fragment, type ReactNode } from 'react'

const INLINE =
  /(`[^`\n]+`)|(\*\*[^*\n]+\*\*|__[^_\n]+__)|(~~[^~\n]+~~)|(\*[^*\s][^*\n]*\*|_[^_\s][^_\n]*_)|(\[[^\]\n]+\]\(https?:\/\/[^\s)]+\))|(https?:\/\/[^\s<>"']+[^\s<>"'.,;:!?)\]])/g

function inline(text: string, key = 0): ReactNode[] {
  const out: ReactNode[] = []
  let last = 0
  for (const m of text.matchAll(INLINE)) {
    if (m.index > last) out.push(text.slice(last, m.index))
    const [tok] = m
    const k = `${key}-${m.index}`
    if (m[1]) out.push(<code key={k}>{tok.slice(1, -1)}</code>)
    else if (m[2]) out.push(<strong key={k}>{inline(tok.slice(2, -2), m.index)}</strong>)
    else if (m[3]) out.push(<s key={k}>{inline(tok.slice(2, -2), m.index)}</s>)
    else if (m[4]) out.push(<em key={k}>{inline(tok.slice(1, -1), m.index)}</em>)
    else if (m[5]) {
      const close = tok.indexOf('](')
      out.push(
        <a key={k} href={tok.slice(close + 2, -1)} target="_blank" rel="noreferrer">
          {tok.slice(1, close)}
        </a>,
      )
    } else if (m[6])
      out.push(
        <a key={k} href={tok} target="_blank" rel="noreferrer">
          {tok}
        </a>,
      )
    last = m.index + tok.length
  }
  if (last < text.length) out.push(text.slice(last))
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

const lines = (ls: string[]) =>
  ls.map((l, i) => (
    <Fragment key={i}>
      {i > 0 && <br />}
      {inline(l, i)}
    </Fragment>
  ))

export function Markdown({ text }: { text: string }) {
  const blocks = parse(text)
  // Plain single-paragraph messages stay inline so the timestamp can float beside them.
  if (blocks.length === 1 && blocks[0].kind === 'p') return <>{lines(blocks[0].lines)}</>
  return (
    <div className="md">
      {blocks.map((b, i) => {
        switch (b.kind) {
          case 'p':
            return <p key={i}>{lines(b.lines)}</p>
          case 'h':
            return (
              <p key={i} className="md-h">
                {inline(b.text)}
              </p>
            )
          case 'ul':
          case 'ol': {
            const List = b.kind
            return (
              <List key={i}>
                {b.items.map((it, j) => (
                  <li key={j}>{inline(it, j)}</li>
                ))}
              </List>
            )
          }
          case 'quote':
            return <blockquote key={i}>{lines(b.lines)}</blockquote>
          case 'code':
            return <pre key={i}>{b.text}</pre>
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
