// The parts of „Darstellung“ (docs/settings.md): preview, design, accent and view. The
// settings show the choices as lists, the setup of the first start in one row (`row`).
// Every change applies at once and is saved by the main process.

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
const LAYOUTS: { layout: Layout; label: string; sub?: string }[] = [
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

interface ChoiceProps {
  /** The id of the heading that names the radio group. */
  labelledBy: string
  /** Equally wide buttons in one row, without subtitles. */
  row?: boolean
}

/** One radio group as a row of buttons. */
function ChoiceRow<T extends string>(props: {
  labelledBy: string
  options: { value: T; label: string }[]
  value: T
  onChoose: (value: T) => void
}) {
  return (
    <div className="choice-row" role="radiogroup" aria-labelledby={props.labelledBy}>
      {props.options.map((o) => (
        <button
          key={o.value}
          role="radio"
          aria-checked={props.value === o.value}
          className={`choice-pill${props.value === o.value ? ' on' : ''}`}
          onClick={() => props.onChoose(o.value)}
        >
          {o.label}
        </button>
      ))}
    </div>
  )
}

/** „Design“: System, Hell, Dunkel. */
export function ThemeChoice({ labelledBy, row }: ChoiceProps) {
  const { theme, accent } = useAppearance()
  if (row) {
    return (
      <ChoiceRow
        labelledBy={labelledBy}
        options={THEMES.map((t) => ({ value: t.theme, label: t.label }))}
        value={theme}
        onChoose={(t) => setAppearance(t, accent)}
      />
    )
  }
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
export function LayoutChoice({ labelledBy, row }: ChoiceProps) {
  const { layout } = useAppearance()
  if (row) {
    return (
      <ChoiceRow
        labelledBy={labelledBy}
        options={LAYOUTS.map((l) => ({ value: l.layout, label: l.label }))}
        value={layout}
        onChoose={setLayout}
      />
    )
  }
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
