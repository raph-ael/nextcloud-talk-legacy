// The setup of the first start (docs/settings.md): accent, design and view with the preview
// of „Darstellung“, then the sign-in. It shows only while no account is signed in and the
// setup is not done on this device.

import { useCallback, useEffect, useState } from 'react'
import { AccentSwatches, AppearancePreview, LayoutChoice, ThemeChoice } from './AppearanceControls'
import { Login } from './Login'
import appIcon from '../../../../resources/icons/128x128.png'

export function Welcome() {
  const [step, setStep] = useState<1 | 2>(1)
  const back = useCallback(() => setStep(1), [])

  function next() {
    // Done from here on: the next start shows the sign-in alone.
    void window.talk.finishSetup()
    setStep(2)
  }

  return (
    <div className="welcome">
      <div className="welcome-body">
        {step === 1 ? <AppearanceStep onNext={next} /> : <Login onBack={back} />}
        <div className="steps" role="img" aria-label={`Schritt ${step} von 2`}>
          <span className={step === 1 ? 'on' : undefined} />
          <span className={step === 2 ? 'on' : undefined} />
        </div>
      </div>
    </div>
  )
}

/** Step 1: every choice applies and is saved at once, like in „Darstellung“. */
function AppearanceStep({ onNext }: { onNext: () => void }) {
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
      <p className="muted">Such dir aus, wie Wolkenfunk aussehen soll.</p>
      <AppearancePreview />
      <section className="setup-group">
        <h2 id="setup-accent">Akzentfarbe</h2>
        <AccentSwatches labelledBy="setup-accent" />
      </section>
      <section className="setup-group">
        <h2 id="setup-theme">Design</h2>
        <ThemeChoice labelledBy="setup-theme" row />
      </section>
      <section className="setup-group">
        <h2 id="setup-layout">Ansicht</h2>
        <LayoutChoice labelledBy="setup-layout" row />
      </section>
      <button className="button-primary" autoFocus onClick={onNext}>
        Weiter
      </button>
      <p className="muted setup-note">Das kannst du später in den Einstellungen unter „Darstellung“ ändern.</p>
    </div>
  )
}
