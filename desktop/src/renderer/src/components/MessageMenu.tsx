import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from 'react'

export interface MenuItem {
  label: string
  onSelect: () => void
  danger?: boolean
}

/**
 * Context menu at a screen position; arrow keys move, Enter selects, Esc/outside click closes.
 * `header` is an optional top row (the reaction bar).
 */
export function MessageMenu({
  x,
  y,
  items,
  header,
  onClose,
}: {
  x: number
  y: number
  items: MenuItem[]
  header?: ReactNode
  onClose: () => void
}) {
  const ref = useRef<HTMLDivElement>(null)
  const [pos, setPos] = useState({ left: x, top: y })

  // Keep it inside the window.
  useLayoutEffect(() => {
    const el = ref.current
    if (!el) return
    const r = el.getBoundingClientRect()
    setPos({
      left: Math.max(8, Math.min(x, window.innerWidth - r.width - 8)),
      top: Math.max(8, y + r.height > window.innerHeight - 8 ? y - r.height : y),
    })
    const first = el.querySelector<HTMLButtonElement>('.menu-item') ?? el.querySelector<HTMLButtonElement>('button')
    first?.focus()
  }, [x, y])

  useEffect(() => {
    function onDown(e: MouseEvent) {
      if (!ref.current?.contains(e.target as Node)) onClose()
    }
    function onKey(e: KeyboardEvent) {
      const buttons = [...(ref.current?.querySelectorAll<HTMLButtonElement>('button') ?? [])]
      const i = buttons.indexOf(document.activeElement as HTMLButtonElement)
      if (e.key === 'Escape') {
        e.preventDefault()
        e.stopPropagation()
        onClose()
      } else if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
        e.preventDefault()
        const next = (i + (e.key === 'ArrowDown' ? 1 : -1) + buttons.length) % buttons.length
        buttons[next]?.focus()
      }
    }
    window.addEventListener('mousedown', onDown, true)
    window.addEventListener('keydown', onKey, true)
    window.addEventListener('blur', onClose)
    window.addEventListener('resize', onClose)
    return () => {
      window.removeEventListener('mousedown', onDown, true)
      window.removeEventListener('keydown', onKey, true)
      window.removeEventListener('blur', onClose)
      window.removeEventListener('resize', onClose)
    }
  }, [onClose])

  return (
    <div className="menu context-menu" role="menu" ref={ref} style={pos}>
      {header && <div className="menu-header">{header}</div>}
      {items.map((item) => (
        <button
          key={item.label}
          role="menuitem"
          className={`menu-item${item.danger ? ' danger' : ''}`}
          onClick={() => {
            onClose()
            item.onSelect()
          }}
        >
          {item.label}
        </button>
      ))}
    </div>
  )
}
