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
}

export function Avatar({ name, size = 48, token, version = '', userId }: Props) {
  const [failed, setFailed] = useState(false)
  const style = { width: size, height: size, fontSize: size * 0.4 }
  const src = token ? window.talk.avatarUrl(token, version) : userId ? window.talk.userAvatarUrl(userId) : null
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
