import { useEffect, useState, type FormEvent } from 'react'
import { cancelAddAccount } from '../store'
import { toast } from '../call/store'
import appIcon from '../../../../resources/icons/128x128.png'

/**
 * The first sign-in, „Konto hinzufügen“ (`adding`) while the other accounts go on, and the
 * second step of the setup of the first start (`onBack`, docs/settings.md).
 */
export function Login({ adding = false, onBack }: { adding?: boolean; onBack?: () => void }) {
  const [server, setServer] = useState('')
  const [waiting, setWaiting] = useState(false)
  const [error, setError] = useState<string | null>(null)

  async function submit(e: FormEvent) {
    e.preventDefault()
    if (!server.trim() || waiting) return
    setError(null)
    setWaiting(true)
    try {
      const result = await window.talk.addAccount(server.trim())
      if (result === 'duplicate') toast('Dieses Konto ist schon angemeldet')
      // Back to the chats (a new account closes this already, by switching to it).
      if (adding) cancelAddAccount()
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err)
      if (!msg.includes('CANCELLED')) {
        setError(
          msg.includes('NETWORK')
            ? 'Server nicht erreichbar. Stimmt die Adresse?'
            : msg.includes('LOGIN_EXPIRED')
              ? 'Die Anmeldung ist abgelaufen. Bitte erneut versuchen.'
              : 'Anmeldung fehlgeschlagen.',
        )
      }
    } finally {
      setWaiting(false)
    }
  }

  // Esc leads back like „Abbrechen“ (to the chats) and „Zurück“ (to the setup; not while waiting).
  const back = adding ? cancelAddAccount : waiting ? undefined : onBack
  useEffect(() => {
    if (!back) return
    function onKey(e: KeyboardEvent) {
      if (e.key === 'Escape' && !e.defaultPrevented) back?.()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [back])

  return (
    <div className="login">
      <form className="login-card" onSubmit={submit}>
        <img className="login-logo" src={appIcon} width={64} height={64} alt="" aria-hidden draggable={false} />
        <h1>{adding ? 'Konto hinzufügen' : onBack ? 'Anmelden' : 'Willkommen'}</h1>
        {waiting ? (
          <>
            <p className="muted">Bestätige die Anmeldung im Browser-Fenster, das sich gerade geöffnet hat.</p>
            <div className="spinner" aria-label="Warte auf Bestätigung" />
            <button type="button" className="button-secondary" onClick={() => void window.talk.cancelLogin()}>
              Abbrechen
            </button>
          </>
        ) : (
          <>
            <p className="muted">Melde dich mit deinem Nextcloud-Konto an.</p>
            <input
              className="input"
              autoFocus
              placeholder="cloud.example.org"
              value={server}
              onChange={(e) => setServer(e.target.value)}
              spellCheck={false}
              autoCapitalize="off"
            />
            <button className="button-primary" type="submit" disabled={!server.trim()}>
              Anmelden
            </button>
            {error && <p className="error">{error}</p>}
            {adding ? (
              <button type="button" className="button-secondary" onClick={cancelAddAccount}>
                Abbrechen
              </button>
            ) : (
              onBack && (
                <button type="button" className="button-secondary" onClick={onBack}>
                  Zurück
                </button>
              )
            )}
          </>
        )}
      </form>
    </div>
  )
}
