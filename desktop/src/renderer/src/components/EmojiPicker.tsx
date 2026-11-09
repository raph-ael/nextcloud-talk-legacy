import { useEffect, useRef } from 'react'
import data from '@emoji-mart/data'
import de from '@emoji-mart/data/i18n/de.json'
import { Picker } from 'emoji-mart'

interface Props {
  onPick: (emoji: string) => void
  onClose: () => void
  /** Clicks on this element (the toggle button) do not count as "outside". */
  anchor: React.RefObject<HTMLElement | null>
}

/** Emoji panel above the composer (emoji-mart, data bundled, native emoji glyphs). */
export function EmojiPicker({ onPick, onClose, anchor }: Props) {
  const host = useRef<HTMLDivElement>(null)
  const pick = useRef(onPick)
  const close = useRef(onClose)
  pick.current = onPick
  close.current = onClose

  useEffect(() => {
    const picker = new Picker({
      data,
      i18n: de,
      locale: 'de',
      set: 'native',
      theme: 'auto',
      autoFocus: true,
      previewPosition: 'none',
      skinTonePosition: 'search',
      maxFrequentRows: 2,
      perLine: 8,
      emojiSize: 22,
      emojiButtonSize: 34,
      onEmojiSelect: (e: { native: string }) => pick.current(e.native),
      onClickOutside: (e: MouseEvent) => {
        if (!anchor.current?.contains(e.target as Node)) close.current()
      },
    }) as unknown as HTMLElement
    host.current?.appendChild(picker)
    return () => picker.remove()
  }, [anchor])

  return (
    <div
      className="emoji-popover"
      ref={host}
      onKeyDown={(e) => {
        if (e.key === 'Escape') {
          e.stopPropagation()
          close.current()
        }
      }}
    />
  )
}
