import { useState } from 'react'
import type { FileAttachment } from '@shared/core'
import type { UiMessage } from '../store'
import { fileSize } from '../format'

/** Width of photos in chat bubbles (CSS px). */
export const IMAGE_WIDTH = 300
const MIN_HEIGHT = 120
const MAX_HEIGHT = 340

export function isImageAttachment(a: FileAttachment | null | undefined): a is FileAttachment {
  return !!a && a.mimetype.startsWith('image/') && !a.voiceMessage && (a.previewAvailable || !a.id)
}

/** Preview for the bubble or the viewer; pending attachments come from the local copy. */
export function imageSrc(msg: UiMessage, size: 'bubble' | 'full'): string {
  const a = msg.attachment!
  if (!a.id && msg.localPath) return window.talk.localFileUrl(msg.localPath)
  // Requested at 2× for HiDPI screens; the core caches every size on disk.
  return size === 'bubble'
    ? window.talk.previewUrl(a.id, IMAGE_WIDTH * 2, MAX_HEIGHT * 2)
    : window.talk.previewUrl(a.id, 2048, 2048)
}

/**
 * Width/height of loaded previews, by file id and by reference id (so the server message
 * replacing a pending photo keeps its size). The server often has no image dimensions.
 */
const aspects = new Map<string, number>()

function knownAspect(msg: UiMessage): number | undefined {
  const a = msg.attachment!
  if (a.width > 0 && a.height > 0) return a.width / a.height
  return (a.id && aspects.get(a.id)) || (msg.referenceId && aspects.get(msg.referenceId)) || undefined
}

function heightFor(aspect: number): number {
  return Math.round(Math.min(MAX_HEIGHT, Math.max(MIN_HEIGHT, IMAGE_WIDTH / aspect)))
}

/** A photo inside a bubble; its size comes from the known dimensions, so nothing jumps. */
export function ChatImage({
  msg,
  onOpen,
  children,
}: {
  msg: UiMessage
  onOpen: () => void
  children?: React.ReactNode
}) {
  const a = msg.attachment!
  const uploading = msg.status && (msg.progress ?? -1) >= 0
  const [aspect, setAspect] = useState(() => knownAspect(msg) ?? 1)
  return (
    <button
      className="chat-image"
      style={{ height: heightFor(aspect) }}
      onClick={onOpen}
      aria-label={`Bild ${a.name} öffnen`}
    >
      <img
        src={imageSrc(msg, 'bubble')}
        alt=""
        draggable={false}
        loading="lazy"
        onLoad={(e) => {
          const img = e.currentTarget
          if (!img.naturalWidth || !img.naturalHeight) return
          const r = img.naturalWidth / img.naturalHeight
          if (a.id) aspects.set(a.id, r)
          if (msg.referenceId) aspects.set(msg.referenceId, r)
          if (!(a.width > 0 && a.height > 0) && Math.abs(r - aspect) > 0.01) setAspect(r)
        }}
      />
      {msg.status === 'sending' && <ProgressRing progress={uploading ? msg.progress! : -1} />}
      {children}
    </button>
  )
}

/** Upload progress over a pending photo or file; indeterminate while not uploading. */
export function ProgressRing({ progress, size = 44 }: { progress: number; size?: number }) {
  const r = size / 2 - 4
  const c = 2 * Math.PI * r
  return (
    <span className="progress-ring" style={{ width: size, height: size }} aria-label="Wird hochgeladen">
      <svg viewBox={`0 0 ${size} ${size}`} width={size} height={size} className={progress < 0 ? 'spin' : ''}>
        <circle cx={size / 2} cy={size / 2} r={r} fill="none" stroke="rgb(255 255 255 / 0.35)" strokeWidth="3" />
        <circle
          cx={size / 2}
          cy={size / 2}
          r={r}
          fill="none"
          stroke="#fff"
          strokeWidth="3"
          strokeLinecap="round"
          strokeDasharray={c}
          strokeDashoffset={progress < 0 ? c * 0.75 : c * (1 - Math.min(1, progress))}
          transform={`rotate(-90 ${size / 2} ${size / 2})`}
        />
      </svg>
    </span>
  )
}

function extension(name: string): string {
  const i = name.lastIndexOf('.')
  return i > 0 && name.length - i <= 5 ? name.slice(i + 1).toLowerCase() : ''
}

/** Document icon with extension, file name and size; a click opens the file in the browser. */
export function FileRow({ msg }: { msg: UiMessage }) {
  const a = msg.attachment!
  const ext = extension(a.name)
  const body = (
    <>
      <span className="file-icon" aria-hidden>
        {msg.status === 'sending' && (msg.progress ?? -1) >= 0 ? (
          <ProgressRing progress={msg.progress!} size={32} />
        ) : (
          <span className={ext.length > 3 ? 'small' : ''}>{ext}</span>
        )}
      </span>
      <span className="file-text">
        <span className="file-name">{a.voiceMessage ? 'Sprachnachricht' : a.name}</span>
        <span className="file-size">{fileSize(a.size)}</span>
      </span>
    </>
  )
  return a.link && !msg.status ? (
    <a className="file-row" href={a.link} target="_blank" rel="noreferrer" title="Im Browser öffnen">
      {body}
    </a>
  ) : (
    <div className="file-row">{body}</div>
  )
}
