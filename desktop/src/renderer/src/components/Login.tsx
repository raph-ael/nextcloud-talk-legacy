import { useEffect, useState, type FormEvent } from 'react'
import { cancelAddAccount } from '../store'
import { toast } from '../call/store'

/** The first sign-in, and „Konto hinzufügen“ (`adding`) while the other accounts go on. */
export function Login({ adding = false }: { adding?: boolean }) {
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

  // Esc leads back to the chats like „Abbrechen“.
  useEffect(() => {
    if (!adding) return
    function onKey(e: KeyboardEvent) {
      if (e.key === 'Escape' && !e.defaultPrevented) cancelAddAccount()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [adding])

  return (
    <div className="login">
      <form className="login-card" onSubmit={submit}>
        <div className="login-logo" aria-hidden>
          <svg viewBox="0 0 64 64" width="64" height="64">
            <rect width="64" height="64" rx="16" fill="var(--accent)" />
            <path
              d="M19 22h20a8 8 0 0 1 8 8v7a8 8 0 0 1-8 8H27l-8 6 1-6a8 8 0 0 1-7-8v-7a8 8 0 0 1 6-8z"
              fill="#fff"
            />
          </svg>
        </div>
        <h1>{adding ? 'Konto hinzufügen' : 'Willkommen'}</h1>
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
            {adding && (
              <button type="button" className="button-secondary" onClick={cancelAddAccount}>
                Abbrechen
              </button>
            )}
          </>
        )}
      </form>
    </div>
  )
}
