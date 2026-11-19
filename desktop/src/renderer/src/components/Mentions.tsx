// Mentions (docs/mentions.md): the suggestions above the input field, the mark behind the
// names in the field, and the mention in a bubble.

import { Fragment, useLayoutEffect, useRef, type ReactNode } from 'react'
import type { MentionCandidate, MentionView } from '@shared/core'
import { Avatar } from './Avatar'
import { PeopleIcon } from './People'

/** The id of a row, for `aria-activedescendant` of the input field. */
export function mentionRowId(listId: string, index: number): string {
  return `${listId}-${index}`
}

interface ListProps {
  id: string
  rows: MentionCandidate[]
  chosen: number
  /** The chat: its picture stands for „Alle“. */
  token: string
  roomName: string
  avatarVersion: string
  onChoose: (index: number) => void
  onAccept: (row: MentionCandidate) => void
}

/** The list above the input field. The focus stays in the field, which also has the keys. */
export function MentionList({ id, rows, chosen, token, roomName, avatarVersion, onChoose, onAccept }: ListProps) {
  const list = useRef<HTMLDivElement>(null)

  // The chosen row stays in sight (arrow keys beyond six rows).
  useLayoutEffect(() => {
    const el = list.current
    const row = el?.children[chosen] as HTMLElement | undefined
    if (!el || !row) return
    const top = row.offsetTop
    const bottom = top + row.offsetHeight
    if (top < el.scrollTop) el.scrollTop = top
    else if (bottom > el.scrollTop + el.clientHeight) el.scrollTop = bottom - el.clientHeight
  }, [chosen, rows])

  return (
    <div
      className="mention-popover"
      id={id}
      ref={list}
      role="listbox"
      aria-label="Erwähnen"
      // A click must not take the focus out of the field.
      onMouseDown={(e) => e.preventDefault()}
    >
      {rows.map((row, i) => (
        <div
          key={row.mentionId}
          id={mentionRowId(id, i)}
          className={`mention-row${i === chosen ? ' chosen' : ''}`}
          role="option"
          aria-selected={i === chosen}
          // Not `mouseenter`: rows moving under a resting pointer (arrow keys) choose nothing.
          onMouseMove={() => i !== chosen && onChoose(i)}
          onClick={() => onAccept(row)}
        >
          {row.kind === 'user' && row.userId ? (
            <Avatar name={row.label} userId={row.userId} size={28} />
          ) : row.kind === 'all' ? (
            <Avatar name={roomName} token={token} version={avatarVersion} size={28} />
          ) : row.kind === 'group' || row.kind === 'team' ? (
            <span className="mention-row-icon" aria-hidden>
              <PeopleIcon size={16} />
            </span>
          ) : (
            <Avatar name={row.label} size={28} />
          )}
          <span className="mention-row-text">
            <span className="mention-row-name">{row.label}</span>
            {row.subline && <span className="mention-row-sub">{row.subline}</span>}
          </span>
        </div>
      ))}
    </div>
  )
}

/**
 * The layer behind the text of the input field: the same text, invisible, with the mentions
 * marked. The mark changes neither width nor weight, so both layers break alike.
 */
export function MentionBackdrop({ text, ranges }: { text: string; ranges: Array<[number, number]> }) {
  const parts: ReactNode[] = []
  let pos = 0
  for (const [from, to] of ranges) {
    if (from > pos) parts.push(<Fragment key={pos}>{text.slice(pos, from)}</Fragment>)
    parts.push(
      <span key={from} className="mention">
        {text.slice(from, to)}
      </span>,
    )
    pos = to
  }
  // The field shows an empty last line after a line break; the space gives this layer one too.
  parts.push(<Fragment key={pos}>{`${text.slice(pos)} `}</Fragment>)
  return <>{parts}</>
}

/** What a bubble needs to draw its mentions. */
export interface BubbleMentions {
  list: MentionView[]
  /** A foreign bubble: a mention of the user is marked in the accent. */
  foreign: boolean
  onOpen: (mention: MentionView) => void
}

/** A mention in a bubble; one of another person opens the chat with them. */
export function Mention({ mention, ctx, children }: { mention: MentionView; ctx: BubbleMentions; children: ReactNode }) {
  const self = mention.isSelf && ctx.foreign
  const link = mention.kind === 'user' && !mention.isSelf
  const cls = `mention${self ? ' self' : ''}${link ? ' person' : ''}`
  if (!link) return <span className={cls}>{children}</span>
  return (
    <span
      className={cls}
      role="link"
      tabIndex={0}
      aria-label={`Chat mit ${mention.label.replace(/^@/, '')} öffnen`}
      onClick={(e) => {
        // Not the bubble's click (send again) or double click (reply).
        e.stopPropagation()
        ctx.onOpen(mention)
      }}
      onDoubleClick={(e) => e.stopPropagation()}
      onKeyDown={(e) => {
        if (e.key !== 'Enter') return
        e.preventDefault()
        ctx.onOpen(mention)
      }}
    >
      {children}
    </span>
  )
}
