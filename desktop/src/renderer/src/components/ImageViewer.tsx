import { useEffect, useRef, useState, type PointerEvent, type WheelEvent } from 'react'
import type { UiMessage } from '../store'
import { fullTime } from '../format'
import { imageSrc } from './ChatMedia'

/** Full-window photo view: wheel/double-click zoom, drag to pan, Esc or a click beside it closes. */
export function ImageViewer({ msg, onClose }: { msg: UiMessage; onClose: () => void }) {
  const a = msg.attachment!
  const [scale, setScale] = useState(1)
  const [offset, setOffset] = useState({ x: 0, y: 0 })
  // The bubble's preview is cached: show it at once, swap in the large one once loaded.
  const [src, setSrc] = useState(() => imageSrc(msg, 'bubble'))
  const drag = useRef<{ x: number; y: number; ox: number; oy: number; moved: boolean } | null>(null)

  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if (e.key === 'Escape') {
        e.stopPropagation()
        onClose()
      }
    }
    window.addEventListener('keydown', onKey, true)
    return () => window.removeEventListener('keydown', onKey, true)
  }, [onClose])

  function zoomTo(next: number) {
    const s = Math.min(6, Math.max(1, next))
    setScale(s)
    if (s === 1) setOffset({ x: 0, y: 0 })
  }

  function onWheel(e: WheelEvent) {
    zoomTo(scale * (e.deltaY < 0 ? 1.2 : 1 / 1.2))
  }

  function onPointerDown(e: PointerEvent) {
    if (scale === 1) return
    e.currentTarget.setPointerCapture(e.pointerId)
    drag.current = { x: e.clientX, y: e.clientY, ox: offset.x, oy: offset.y, moved: false }
  }
  function onPointerMove(e: PointerEvent) {
    const d = drag.current
    if (!d) return
    d.moved = true
    setOffset({ x: d.ox + (e.clientX - d.x) / scale, y: d.oy + (e.clientY - d.y) / scale })
  }

  useEffect(() => {
    const large = imageSrc(msg, 'full')
    const img = new Image()
    img.onload = () => setSrc(large)
    img.src = large
    return () => {
      img.onload = null
    }
  }, [msg])

  return (
    <div className="viewer" role="dialog" aria-label="Bild" onClick={onClose} onWheel={onWheel}>
      <div className="viewer-bar" onClick={(e) => e.stopPropagation()}>
        <div className="viewer-title">
          <strong>{msg.isOwn ? 'Du' : msg.author}</strong>
          <span>{fullTime(msg.timestamp)}</span>
        </div>
        {a.link && (
          <a className="viewer-button" href={a.link} target="_blank" rel="noreferrer" title="Im Browser öffnen">
            <svg viewBox="0 0 24 24" width="20" height="20">
              <path fill="currentColor" d="M14 3h7v7h-2V6.4l-9.3 9.3-1.4-1.4L17.6 5H14zM5 5h6v2H5v12h12v-6h2v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7c0-1.1.9-2 2-2z" />
            </svg>
          </a>
        )}
        <button className="viewer-button" onClick={onClose} aria-label="Schließen" title="Schließen (Esc)">
          <svg viewBox="0 0 24 24" width="20" height="20">
            <path fill="currentColor" d="M18.3 5.7 12 12l6.3 6.3-1.4 1.4L12 13.4l-6.3 6.3-1.4-1.4L10.6 12 4.3 5.7l1.4-1.4L12 10.6l6.3-6.3z" />
          </svg>
        </button>
      </div>
      <div className="viewer-stage">
        <img
          src={src}
          alt={a.name}
          draggable={false}
          className={scale > 1 ? 'zoomed' : ''}
          style={{ transform: `scale(${scale}) translate(${offset.x}px, ${offset.y}px)` }}
          onClick={(e) => {
            e.stopPropagation()
            if (drag.current?.moved) return
          }}
          onDoubleClick={(e) => {
            e.stopPropagation()
            zoomTo(scale > 1 ? 1 : 2.5)
          }}
          onPointerDown={onPointerDown}
          onPointerMove={onPointerMove}
          onPointerUp={() => setTimeout(() => (drag.current = null))}
        />
      </div>
    </div>
  )
}
