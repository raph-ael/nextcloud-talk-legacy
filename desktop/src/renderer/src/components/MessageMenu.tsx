import { useCallback, useEffect, useLayoutEffect, useRef, useState, type ReactNode } from 'react'

/**
 * One entry. Plain entries close the menu and run `onSelect`. `check` and `radio` show their
 * state (`keepOpen`: the menu stays, e.g. toggling categories); `submenu` opens on hover or →.
 */
export type MenuItem =
  | { kind?: 'action'; label: string; onSelect: () => void; danger?: boolean }
  | { kind: 'check'; label: string; checked: boolean; onSelect: () => void; keepOpen?: boolean }
  | { kind: 'radio'; label: string; checked: boolean; onSelect: () => void }
  | { kind: 'submenu'; label: string; items: MenuItem[] }
  | { kind: 'separator' }

/**
 * Context menu at a screen position; ↑/↓ move, Enter selects, → opens a submenu and ← or Esc
 * closes it, Esc/outside click closes the menu. `header` is an optional top row (reactions).
 */
export function MessageMenu({
  x,
  y,
  items,
  header,
  label,
  onClose,
}: {
  x: number
  y: number
  items: MenuItem[]
  header?: ReactNode
  /** Accessible name of the menu. */
  label?: string
  onClose: () => void
}) {
  const layer = useRef<HTMLDivElement>(null)
  // Index of the open submenu; `focus` puts the focus on its first entry (keyboard).
  const [open, setOpen] = useState<{ index: number; focus: boolean } | null>(null)
  const anchors = useRef(new Map<number, HTMLButtonElement>())
  const close = useRef(onClose)
  close.current = onClose
  const openRef = useRef(open)
  openRef.current = open

  // An open submenu whose entry is gone (e.g. the list changed) closes.
  const sub = open ? items[open.index] : undefined
  const subItems = sub?.kind === 'submenu' ? sub.items : null

  useEffect(() => {
    function onDown(e: MouseEvent) {
      if (!layer.current?.contains(e.target as Node)) close.current()
    }
    function onKey(e: KeyboardEvent) {
      const active = document.activeElement as HTMLElement | null
      const list = active?.closest<HTMLElement>('[role="menu"]')
      if (!layer.current || !list || !layer.current.contains(list)) {
        if (e.key === 'Escape') {
          e.preventDefault()
          e.stopPropagation()
          close.current()
        }
        return
      }
      const inSub = list.dataset.level === '1'
      // The reaction bar of a message menu belongs to the top level.
      const entries = [...list.querySelectorAll<HTMLButtonElement>(':scope > button, :scope > .menu-header button')]
      const i = entries.indexOf(active as HTMLButtonElement)
      if (e.key === 'Escape' || (e.key === 'ArrowLeft' && inSub)) {
        e.preventDefault()
        e.stopPropagation()
        if (inSub) {
          const o = openRef.current
          if (o) anchors.current.get(o.index)?.focus()
          setOpen(null)
        } else if (e.key === 'Escape') close.current()
      } else if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
        e.preventDefault()
        e.stopPropagation()
        const next = (i + (e.key === 'ArrowDown' ? 1 : -1) + entries.length) % entries.length
        entries[next]?.focus()
      } else if (e.key === 'ArrowRight' && !inSub && active?.getAttribute('aria-haspopup') === 'menu') {
        e.preventDefault()
        e.stopPropagation()
        setOpen({ index: Number(active.dataset.index), focus: true })
      } else if (e.key === 'Home' || e.key === 'End') {
        e.preventDefault()
        ;(e.key === 'Home' ? entries[0] : entries[entries.length - 1])?.focus()
      }
    }
    const onBlur = () => close.current()
    window.addEventListener('mousedown', onDown, true)
    window.addEventListener('keydown', onKey, true)
    window.addEventListener('blur', onBlur)
    window.addEventListener('resize', onBlur)
    return () => {
      window.removeEventListener('mousedown', onDown, true)
      window.removeEventListener('keydown', onKey, true)
      window.removeEventListener('blur', onBlur)
      window.removeEventListener('resize', onBlur)
    }
  }, [])

  const select = useCallback(
    (item: MenuItem) => {
      if (item.kind === 'separator' || item.kind === 'submenu') return
      if (!(item.kind === 'check' && item.keepOpen)) close.current()
      item.onSelect()
    },
    [],
  )

  return (
    <div className="menu-layer" ref={layer}>
      <MenuList
        level={0}
        items={items}
        header={header}
        label={label}
        at={{ left: x, top: y }}
        autoFocus
        openIndex={subItems ? (open?.index ?? null) : null}
        anchors={anchors.current}
        onHover={(index, isSub) => setOpen(isSub ? { index, focus: false } : null)}
        onOpen={(index) => setOpen((o) => (o?.index === index ? o : { index, focus: true }))}
        onSelect={select}
      />
      {subItems && open && (
        <MenuList
          key={open.index}
          level={1}
          items={subItems}
          label={sub?.kind === 'submenu' ? sub.label : undefined}
          anchor={anchors.current.get(open.index) ?? null}
          autoFocus={open.focus}
          anchors={null}
          onSelect={select}
        />
      )}
    </div>
  )
}

function MenuList({
  level,
  items,
  header,
  label,
  at,
  anchor,
  autoFocus,
  openIndex = null,
  anchors,
  onHover,
  onOpen,
  onSelect,
}: {
  level: 0 | 1
  items: MenuItem[]
  header?: ReactNode
  label?: string
  /** Top level: the click position. */
  at?: { left: number; top: number }
  /** Submenu: the entry it belongs to. */
  anchor?: HTMLElement | null
  autoFocus: boolean
  openIndex?: number | null
  anchors: Map<number, HTMLButtonElement> | null
  onHover?: (index: number, isSubmenu: boolean) => void
  onOpen?: (index: number) => void
  onSelect: (item: MenuItem) => void
}) {
  const ref = useRef<HTMLDivElement>(null)
  const [pos, setPos] = useState<{ left: number; top: number }>(at ?? { left: -9999, top: 0 })

  // Keep it inside the window; a submenu goes right of its entry, else left of the menu.
  useLayoutEffect(() => {
    const el = ref.current
    if (!el) return
    const r = el.getBoundingClientRect()
    if (at) {
      setPos({
        left: Math.max(8, Math.min(at.left, window.innerWidth - r.width - 8)),
        top: Math.max(8, at.top + r.height > window.innerHeight - 8 ? at.top - r.height : at.top),
      })
    } else if (anchor) {
      const a = anchor.getBoundingClientRect()
      const parent = anchor.parentElement!.getBoundingClientRect()
      const right = parent.right - 2
      const left = right + r.width > window.innerWidth - 8 ? Math.max(8, parent.left - r.width + 2) : right
      setPos({ left, top: Math.max(8, Math.min(a.top - 7, window.innerHeight - r.height - 8)) })
    }
    if (autoFocus) el.querySelector<HTMLButtonElement>(':scope > button')?.focus()
    // Only when it opens (or the click position changes), not on every new item list.
  }, [at?.left, at?.top, anchor])

  // Entries without a mark line up with the ones that have one.
  const marks = items.some((i) => i.kind === 'check' || i.kind === 'radio')

  return (
    <div className="menu context-menu" role="menu" aria-label={label} data-level={level} ref={ref} style={pos}>
      {header && <div className="menu-header">{header}</div>}
      {items.map((item, index) => {
        if (item.kind === 'separator') return <div key={`sep${index}`} className="menu-separator" role="separator" />
        const isSub = item.kind === 'submenu'
        const checkable = item.kind === 'check' || item.kind === 'radio'
        return (
          <button
            key={`${index}:${item.label}`}
            ref={(el) => {
              if (!anchors) return
              if (el) anchors.set(index, el)
              else anchors.delete(index)
            }}
            data-index={index}
            role={item.kind === 'check' ? 'menuitemcheckbox' : item.kind === 'radio' ? 'menuitemradio' : 'menuitem'}
            aria-checked={checkable ? item.checked : undefined}
            aria-haspopup={isSub ? 'menu' : undefined}
            aria-expanded={isSub ? openIndex === index : undefined}
            className={`menu-item${item.kind !== 'check' && item.kind !== 'radio' && !isSub && item.danger ? ' danger' : ''}${
              checkable ? ' checkable' : ''
            }${isSub ? ' has-sub' : ''}${isSub && openIndex === index ? ' open' : ''}`}
            onMouseEnter={() => onHover?.(index, isSub)}
            onClick={() => (isSub ? onOpen?.(index) : onSelect(item))}
          >
            {(checkable || marks) && (
              <span className="menu-mark" aria-hidden>
                {checkable &&
                  item.checked &&
                  (item.kind === 'check' ? (
                    <svg viewBox="0 0 16 16" width="14" height="14">
                      <path fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" d="M3.5 8.5l3 3 6-7" />
                    </svg>
                  ) : (
                    <span className="menu-dot" />
                  ))}
              </span>
            )}
            <span className="menu-label">{item.label}</span>
            {isSub && (
              <svg className="menu-chevron" viewBox="0 0 16 16" width="12" height="12" aria-hidden>
                <path fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" d="M6 3.5l4.5 4.5L6 12.5" />
              </svg>
            )}
          </button>
        )
      })}
    </div>
  )
}
