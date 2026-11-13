import { useEffect, useRef, type ReactNode } from 'react'

/**
 * Centered modal over the window (pattern of „Wer hat reagiert“): Esc and a click on the
 * backdrop close it. The first element with `data-autofocus` (else the dialog) gets the focus;
 * Tab stays inside.
 */
export function Modal({
  label,
  className = '',
  onClose,
  role = 'dialog',
  children,
}: {
  label: string
  className?: string
  onClose: () => void
  role?: 'dialog' | 'alertdialog'
  children: ReactNode
}) {
  const ref = useRef<HTMLDivElement>(null)
  const close = useRef(onClose)
  close.current = onClose

  useEffect(() => {
    const el = ref.current
    const before = document.activeElement as HTMLElement | null
    ;(el?.querySelector<HTMLElement>('[data-autofocus]') ?? el)?.focus()
    function onKey(e: KeyboardEvent) {
      // Only the topmost modal reacts.
      const all = document.querySelectorAll('.modal')
      if (all[all.length - 1] !== el) return
      if (e.key === 'Escape') {
        e.preventDefault()
        e.stopPropagation()
        close.current()
      } else if (e.key === 'Tab' && el) {
        const items = [
          ...el.querySelectorAll<HTMLElement>('button:not(:disabled), input:not(:disabled), textarea:not(:disabled), [tabindex="0"]'),
        ]
        if (items.length === 0) return
        const first = items[0]
        const last = items[items.length - 1]
        if (e.shiftKey && (document.activeElement === first || document.activeElement === el)) {
          e.preventDefault()
          last.focus()
        } else if (!e.shiftKey && document.activeElement === last) {
          e.preventDefault()
          first.focus()
        }
      }
    }
    window.addEventListener('keydown', onKey, true)
    return () => {
      window.removeEventListener('keydown', onKey, true)
      if (before?.isConnected) before.focus()
    }
  }, [])

  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && close.current()}>
      <div className={`modal ${className}`} role={role} aria-modal="true" aria-label={label} tabIndex={-1} ref={ref}>
        {children}
      </div>
    </div>
  )
}

/** Confirmation in the style of the call dialogs (title, text, „Abbrechen“ + action). */
export function ConfirmDialog({
  title,
  text,
  confirm,
  cancel = 'Abbrechen',
  danger = false,
  busy = false,
  onConfirm,
  onCancel,
}: {
  title: string
  text?: string
  confirm: string
  cancel?: string
  danger?: boolean
  busy?: boolean
  onConfirm: () => void
  onCancel: () => void
}) {
  return (
    <Modal label={title} className="dialog" role="alertdialog" onClose={busy ? () => {} : onCancel}>
      <h2 className="dialog-title">{title}</h2>
      {text && <p className="dialog-text">{text}</p>}
      <div className="dialog-buttons">
        {/* Destructive actions start on „Abbrechen“ so Enter does not delete by accident. */}
        <button className="dialog-btn" onClick={onCancel} disabled={busy} data-autofocus={danger || undefined}>
          {cancel}
        </button>
        <button
          className={`dialog-btn ${danger ? 'danger' : 'primary'}`}
          onClick={onConfirm}
          disabled={busy}
          data-autofocus={!danger || undefined}
        >
          {busy ? <span className="spinner tiny" aria-label="Bitte warten" /> : confirm}
        </button>
      </div>
    </Modal>
  )
}
