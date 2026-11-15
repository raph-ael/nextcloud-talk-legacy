import { useState } from 'react'
import { colorFor, initials } from '../format'

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

export function Avatar({ name, size = 48, token, version = '', userId, account }: Props) {
  const [failed, setFailed] = useState(false)
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
