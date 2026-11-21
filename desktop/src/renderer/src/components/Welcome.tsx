// The setup of the first start (docs/settings.md): one question, the view, then the sign-in. It shows only while no account is signed in and the setup is
// not done on this device.

import { useCallback, useEffect, useState } from 'react'
import type { Layout } from '@shared/core'
import { setLayout, useAppearance } from '../appearance'
import { LAYOUTS } from './AppearanceControls'
import { Login } from './Login'
import appIcon from '../../../../resources/icons/128x128.png'

export function Welcome() {
  const [step, setStep] = useState<1 | 2>(1)
  const toView = useCallback(() => setStep(1), [])
  const toLogin = useCallback(() => {
    // Done from here on: the next start shows the sign-in alone.
    void window.talk.finishSetup()
    setStep(2)
  }, [])

  return (
    <div className="welcome">
      <div className="welcome-body">
        {step === 1 ? <ViewStep onNext={toLogin} /> : <Login onBack={toView} />}
        <div className="steps" role="img" aria-label={`Schritt ${step} von 2`}>
          {[1, 2].map((n) => (
            <span key={n} className={n === step ? 'on' : undefined} />
          ))}
        </div>
      </div>
    </div>
  )
}

/** The view, as two cards with a drawn picture each. */
function ViewStep({ onNext }: { onNext: () => void }) {
  const { layout } = useAppearance()
  // Enter presses „Weiter“ unless another button has the focus (it presses itself).
  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if (e.key !== 'Enter' || e.defaultPrevented || document.activeElement instanceof HTMLButtonElement) return
      e.preventDefault()
      onNext()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [onNext])
  return (
    <div className="login-card setup-card">
      <img className="login-logo" src={appIcon} width={64} height={64} alt="" aria-hidden draggable={false} />
      <h1>Willkommen bei Wolkenfunk</h1>
      <p className="muted">Wie sollen deine Chats aussehen?</p>
      <div className="view-cards" role="radiogroup" aria-label="Ansicht">
        {LAYOUTS.map((l) => (
          <button
            key={l.layout}
            role="radio"
            aria-checked={layout === l.layout}
            className={`view-card${layout === l.layout ? ' on' : ''}`}
            onClick={() => setLayout(l.layout)}
          >
            <ViewArt layout={l.layout} />
            {l.label}
          </button>
        ))}
      </div>
      <button className="button-primary" autoFocus onClick={onNext}>
        Weiter
      </button>
      <p className="muted setup-note">Das kannst du später in den Einstellungen ändern.</p>
    </div>
  )
}

/** Rows of the pictures: their centres from top to bottom. */
const THREE = [24, 54, 84]
const FIVE = [18, 36, 54, 72, 90]

/**
 * A small window in shapes only: the chat list on the left, the chat on the right. All
 * colours are the app's (styles.css), so the pictures follow the design and the accent.
 */
function ViewArt({ layout }: { layout: Layout }) {
  return (
    <svg className="view-art" viewBox="0 0 160 108" aria-hidden>
      <rect className="art-window" x="0.5" y="0.5" width="159" height="107" rx="9" />
      <path className="art-line" d="M52.5 1v106" />
      {layout === 'bubbles' ? (
        <>
          {/* Three chats: picture, name and preview. */}
          {THREE.map((y, i) => (
            <g key={y}>
              <circle className="art-shape" cx="15" cy={y} r="7" />
              <rect className="art-bar strong" x="27" y={y - 6} width={[16, 13, 18][i]} height="4" rx="2" />
              <rect className="art-bar" x="27" y={y + 2} width="20" height="4" rx="2" />
            </g>
          ))}
          {/* A foreign bubble, an own one, a foreign one. */}
          <rect className="art-shape" x="62" y="14" width="58" height="20" rx="10" />
          <rect className="art-own" x="105" y="44" width="46" height="20" rx="10" />
          <rect className="art-shape" x="62" y="74" width="40" height="20" rx="10" />
        </>
      ) : (
        <>
          {/* Five chats, one line each. */}
          {FIVE.map((y, i) => (
            <g key={y}>
              <circle className="art-shape" cx="12" cy={y} r="4" />
              <rect className="art-bar" x="21" y={y - 2} width={[22, 17, 24, 19, 21][i]} height="4" rx="2" />
            </g>
          ))}
          {/* Three messages one under the other: picture, name, text. */}
          {THREE.map((y, i) => (
            <g key={y}>
              <circle className="art-shape" cx="68" cy={y} r="6" />
              <rect className="art-bar strong" x="80" y={y - 6} width={[16, 12, 16][i]} height="4" rx="2" />
              <rect className="art-bar" x="80" y={y + 2} width={[62, 44, 54][i]} height="4" rx="2" />
            </g>
          ))}
        </>
      )}
    </svg>
  )
}
