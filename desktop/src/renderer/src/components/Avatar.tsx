import { useState } from 'react'
import type { RoomView } from '@shared/core'
import { colorFor, initials } from '../format'
import { useStore } from '../store'

interface Props {
  name: string
  size?: number
  /** Room token: loads the server-rendered conversation avatar. */
  token?: string
  version?: string
  /** Nextcloud user id: loads the user's avatar (call participants). */
  userId?: string
  /** Another account than the one shown (a call, the account menu). */
  account?: string
}

/**
 * Tokens of „Notizen an mich“, for the newest list of chats. `hasCustomAvatar` says nothing
 * here: the server sets an emoji picture when it creates the chat and reports it as custom.
 */
let notesOf: RoomView[] | null = null
let notesTokens = new Set<string>()

function isNotes(rooms: RoomView[], token: string): boolean {
  if (rooms !== notesOf) {
    notesOf = rooms
    notesTokens = new Set(rooms.filter((r) => r.roomType === 6).map((r) => r.token))
  }
  return notesTokens.has(token)
}

export function Avatar({ name, size = 48, token, version = '', userId, account }: Props) {
  const [failed, setFailed] = useState(false)
  // „Notizen an mich“ gets its own picture instead of the server's (docs/chat.md). The list
  // of chats is the one of the account shown: a token of another account is left alone.
  const notes = useStore(
    (s) => !!token && (!account || account === s.account?.id) && isNotes(s.rooms, token),
  )
  const style = { width: size, height: size, fontSize: size * 0.4 }
  const src = token
    ? window.talk.avatarUrl(token, version, account)
    : userId
      ? window.talk.userAvatarUrl(userId, account)
      : null
  // Another picture (another account): try it, even if the one before failed.
  const [shownSrc, setShownSrc] = useState(src)
  if (shownSrc !== src) {
    setShownSrc(src)
    setFailed(false)
  }
  if (notes) {
    return (
      <div className="avatar avatar-notes" style={style} aria-hidden>
        <svg
          viewBox="0 0 24 24"
          width={size / 2}
          height={size / 2}
          fill="none"
          stroke="currentColor"
          strokeWidth={1.8}
          strokeLinecap="round"
          strokeLinejoin="round"
        >
          <rect x="5" y="3.5" width="14" height="17" rx="2.5" />
          <path d="M8.5 8.5h7M8.5 12h7M8.5 15.5h4" />
        </svg>
      </div>
    )
  }
  if (src && !failed) {
    return (
      <img
        className="avatar"
        style={style}
        src={src}
        alt=""
        draggable={false}
        onError={() => setFailed(true)}
      />
    )
  }
  return (
    <div className="avatar avatar-initials" style={{ ...style, background: colorFor(name) }} aria-hidden>
      {initials(name)}
    </div>
  )
}
