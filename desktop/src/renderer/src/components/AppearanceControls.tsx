// The parts of „Darstellung“ (docs/settings.md): preview, design, accent and view. The
// setup of the first start shows the preview and the accents too. Every change applies at
// once and is saved by the main process.

import type { Layout, Theme } from '@shared/core'
import { ACCENTS } from '../accents'
import { setAppearance, setLayout, useAppearance } from '../appearance'
import { Avatar } from './Avatar'

const THEMES: { theme: Theme; label: string }[] = [
  { theme: 'system', label: 'System' },
  { theme: 'light', label: 'Hell' },
  { theme: 'dark', label: 'Dunkel' },
]

/** „Ansicht“ (docs/compact-view.md). */
export const LAYOUTS: { layout: Layout; label: string; sub?: string }[] = [
  { layout: 'bubbles', label: 'Sprechblasen' },
  { layout: 'compact', label: 'Kompakt', sub: 'Einzeilige Chatliste, Nachrichten untereinander' },
]

/** The preview in the compact view: two runs with their heads. */
function CompactPreviewRow({ name, text, time }: { name: string; text: string; time: string }) {
  return (
    <div className="msg other first last">
      <div className="msg-avatar">
        <Avatar name={name} size={36} />
      </div>
      <div className="msg-col">
        <div className="msg-head">
          <span className="msg-name">{name}</span>
          <span className="msg-time">{time}</span>
        </div>
        <div className="bubble">{text}</div>
      </div>
    </div>
  )
}

/** A foreign and an own message in the chosen view and colours. */
export function AppearancePreview() {
  const { layout } = useAppearance()
  return (
    <div className="appearance-preview" aria-hidden>
      {layout === 'compact' ? (
        <>
          <CompactPreviewRow name="Lena" time="14:02" text="Wie gefällt dir die neue Farbe?" />
          <CompactPreviewRow name="Du" time="14:03" text="Sieht gut aus!" />
        </>
      ) : (
        <>
          <div className="msg other first last">
            <div className="msg-col">
              <div className="bubble">Wie gefällt dir die neue Farbe?</div>
            </div>
          </div>
          <div className="msg own first last">
            <div className="msg-col">
              <div className="bubble">Sieht gut aus!</div>
            </div>
          </div>
        </>
      )}
    </div>
  )
}

/** „Design“: System, Hell, Dunkel. */
export function ThemeChoice({ labelledBy }: { labelledBy: string }) {
  const { theme, accent } = useAppearance()
  return (
    <div className="choice-list settings-choices" role="radiogroup" aria-labelledby={labelledBy}>
      {THEMES.map((t) => (
        <button key={t.theme} role="radio" aria-checked={theme === t.theme} className="choice" onClick={() => setAppearance(t.theme, accent)}>
          <span className={`radio${theme === t.theme ? ' on' : ''}`} aria-hidden />
          {t.label}
        </button>
      ))}
    </div>
  )
}

/** „Akzentfarbe“: the ten colours, five in a row. */
export function AccentSwatches({ labelledBy }: { labelledBy: string }) {
  const { theme, accent, dark } = useAppearance()
  return (
    <div className="swatches" role="group" aria-labelledby={labelledBy}>
      {ACCENTS.map((a) => {
        const on = a.id === accent
        const color = dark ? a.dark.accent : a.light.accent
        return (
          <button
            key={a.id}
            className={`swatch${on ? ' on' : ''}`}
            style={{ background: color, color }}
            aria-label={on ? `${a.name}, ausgewählt` : a.name}
            title={a.name}
            onClick={() => setAppearance(theme, a.id)}
          >
            {on && (
              <svg viewBox="0 0 24 24" width="20" height="20" aria-hidden>
                <path fill="none" stroke="#fff" strokeWidth="2.6" strokeLinecap="round" strokeLinejoin="round" d="M6 12.5l4 4 8-9" />
              </svg>
            )}
          </button>
        )
      })}
    </div>
  )
}

/** „Ansicht“: Sprechblasen, Kompakt. */
export function LayoutChoice({ labelledBy }: { labelledBy: string }) {
  const { layout } = useAppearance()
  return (
    <div className="choice-list settings-choices" role="radiogroup" aria-labelledby={labelledBy}>
      {LAYOUTS.map((l) => (
        <button key={l.layout} role="radio" aria-checked={layout === l.layout} className="choice" onClick={() => setLayout(l.layout)}>
          <span className={`radio${layout === l.layout ? ' on' : ''}`} aria-hidden />
          <span className="settings-text">
            <span>{l.label}</span>
            {l.sub && <span className="person-sub wrap">{l.sub}</span>}
          </span>
        </button>
      ))}
    </div>
  )
}
