import { useState } from 'react'
import { colorFor, initials } from '../format'

interface Props {
  name: string
  size?: number
  /** Room token: loads the server-rendered conversation avatar. */
  token?: string
  version?: string
}

export function Avatar({ name, size = 48, token, version = '' }: Props) {
  const [failed, setFailed] = useState(false)
  const style = { width: size, height: size, fontSize: size * 0.4 }
  if (token && !failed) {
    return (
      <img
        className="avatar"
        style={style}
        src={window.talk.avatarUrl(token, version)}
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
