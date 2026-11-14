// „Einstellungen“ (docs/settings.md): replace the whole window in two columns – on the left
// profile and rows in the width of the chat list, on the right the chosen page. The chat
// list and an open chat stay mounted underneath (draft, scroll position); a call goes on.

import { useEffect, useRef, useState, type ReactNode } from 'react'
import type { AppInfo, Theme, UpdateCheck } from '@shared/core'
import { closeSettings, setSettingsPage, useStore, type SettingsPage } from '../store'
import { toast } from '../call/store'
import { ACCENTS } from '../accents'
import { setAppearance, useAppearance } from '../appearance'
import { Avatar } from './Avatar'
import { Categories } from './Categories'
import appIcon from '../../../../resources/icons/128x128.png'

const IS_MAC = navigator.platform.startsWith('Mac')

const TITLES: Record<Exclude<SettingsPage, 'categories'>, string> = {
  general: 'Allgemein',
  appearance: 'Darstellung',
  chats: 'Chats',
  about: 'Über Wolkenfunk',
}

const THEMES: { theme: Theme; label: string }[] = [
  { theme: 'system', label: 'System' },
  { theme: 'light', label: 'Hell' },
  { theme: 'dark', label: 'Dunkel' },
]

export function Settings() {
  const page = useStore((s) => s.settings) ?? 'appearance'
  const account = useStore((s) => s.account)
  const [chats, setChats] = useState(false)
  const ref = useRef<HTMLDivElement>(null)

  useEffect(() => {
    let alive = true
    window.talk
      .categoriesSupported()
      .then((ok) => alive && setChats(ok))
      .catch(() => {})
    return () => {
      alive = false
    }
  }, [account])

  // The chosen row gets the focus when the settings open.
  useEffect(() => {
    ref.current?.querySelector<HTMLElement>('.settings-nav-row[aria-current="page"]')?.focus()
  }, [])

  // Esc goes back to the chats; on „Kategorien“ the page handles Esc itself (back to „Chats“).
  useEffect(() => {
    if (page === 'categories') return
    function onKey(e: KeyboardEvent) {
      if (e.key !== 'Escape' || e.defaultPrevented) return
      const active = document.activeElement
      if (active && active !== document.body && !ref.current?.contains(active)) return
      e.preventDefault()
      closeSettings()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [page])

  const rows: { page: SettingsPage; label: string; icon: ReactNode }[] = [
    ...(IS_MAC ? [] : [{ page: 'general' as const, label: 'Allgemein', icon: <GearIcon /> }]),
    { page: 'appearance', label: 'Darstellung', icon: <BrushIcon /> },
    ...(chats ? [{ page: 'chats' as const, label: 'Chats', icon: <ChatIcon /> }] : []),
    { page: 'about', label: 'Über Wolkenfunk', icon: <InfoIcon /> },
  ]
  const selected = page === 'categories' ? 'chats' : page

  return (
    <div className="settings-shell" ref={ref}>
      <aside className="sidebar settings-nav">
        <header className="pane-header">
          <button className="icon-button" onClick={closeSettings} aria-label="Zurück zu den Chats" title="Zurück zu den Chats">
            <svg viewBox="0 0 24 24" width="20" height="20" aria-hidden>
              <path fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" d="M15 5l-7 7 7 7" />
            </svg>
          </button>
          <h2>Einstellungen</h2>
        </header>
        <div className="settings-nav-scroll">
          <div className="settings-profile">
            <Avatar name={account?.user ?? '?'} userId={account?.userId} size={64} />
            <span className="settings-profile-name">{account?.user}</span>
            <span className="muted">{account?.server.replace(/^https?:\/\//, '')}</span>
          </div>
          <nav className="settings-nav-rows" aria-label="Einstellungen">
            {rows.map((r) => (
              <button
                key={r.page}
                className={`settings-nav-row${selected === r.page ? ' active' : ''}`}
                aria-current={selected === r.page ? 'page' : undefined}
                onClick={() => setSettingsPage(r.page)}
              >
                <span className="settings-icon">{r.icon}</span>
                <span className="settings-text">{r.label}</span>
                <Chevron />
              </button>
            ))}
          </nav>
          <div className="settings-nav-foot">
            <button className="settings-nav-row danger" onClick={() => void window.talk.logout()}>
              <span className="settings-text">Abmelden</span>
            </button>
          </div>
        </div>
      </aside>
      {page === 'categories' ? (
        <Categories onClose={() => setSettingsPage('chats')} />
      ) : (
        <section className="chat settings-page" aria-label={TITLES[page]}>
          <header className="pane-header settings-page-header">
            <h2>{TITLES[page]}</h2>
          </header>
          <div className="settings-scroll">
            <div className="settings">
              {page === 'general' && <GeneralPage />}
              {page === 'appearance' && <AppearancePage />}
              {page === 'chats' && <ChatsPage />}
              {page === 'about' && <AboutPage />}
            </div>
          </div>
        </section>
      )}
    </div>
  )
}

/** „Allgemein“ (Linux, Windows): the same setting as the check mark in the tray menu. */
function GeneralPage() {
  const [on, setOn] = useState<boolean | null>(null)
  useEffect(() => {
    let alive = true
    void window.talk.closeToTray().then((v) => alive && setOn(v))
    const off = window.talk.onEvent((e) => e.type === 'closeToTrayChanged' && setOn(e.on))
    return () => {
      alive = false
      off()
    }
  }, [])
  return (
    <section className="settings-section">
      <button
        className="settings-row"
        role="switch"
        aria-checked={!!on}
        disabled={on === null}
        onClick={() => {
          setOn(!on)
          void window.talk.setCloseToTray(!on)
        }}
      >
        <span className="settings-text">
          <span>Beim Schließen im Hintergrund weiterlaufen</span>
          <span className="person-sub wrap">Damit Anrufe und Nachrichten ankommen</span>
        </span>
        <span className={`switch${on ? ' on' : ''}`} aria-hidden />
      </button>
    </section>
  )
}

/** „Darstellung“: preview, design and accent; every change applies at once. */
function AppearancePage() {
  const { theme, accent, dark } = useAppearance()
  return (
    <>
      <div className="appearance-preview" aria-hidden>
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
      </div>
      <section className="settings-section">
        <h3 className="settings-heading" id="settings-theme">
          Design
        </h3>
        <div className="choice-list settings-choices" role="radiogroup" aria-labelledby="settings-theme">
          {THEMES.map((t) => (
            <button
              key={t.theme}
              role="radio"
              aria-checked={theme === t.theme}
              className="choice"
              onClick={() => setAppearance(t.theme, accent)}
            >
              <span className={`radio${theme === t.theme ? ' on' : ''}`} aria-hidden />
              {t.label}
            </button>
          ))}
        </div>
      </section>
      <section className="settings-section">
        <h3 className="settings-heading" id="settings-accent">
          Akzentfarbe
        </h3>
        <div className="swatches" role="group" aria-labelledby="settings-accent">
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
      </section>
    </>
  )
}

/** „Chats“ (only with server categories): the existing page „Kategorien“. */
function ChatsPage() {
  return (
    <section className="settings-section">
      <button className="settings-row" onClick={() => setSettingsPage('categories')}>
        <span className="settings-text">Kategorien verwalten</span>
        <Chevron />
      </button>
    </section>
  )
}

type UpdateState = { state: 'idle' } | { state: 'checking' } | UpdateCheck

/** „Über Wolkenfunk“: version, core, update check where the app updates itself, links. */
function AboutPage() {
  const [info, setInfo] = useState<AppInfo | null>(null)
  const [update, setUpdate] = useState<UpdateState>({ state: 'idle' })
  useEffect(() => {
    let alive = true
    void window.talk.appInfo().then((i) => alive && setInfo(i))
    return () => {
      alive = false
    }
  }, [])

  async function check() {
    setUpdate({ state: 'checking' })
    try {
      setUpdate(await window.talk.checkForUpdates())
    } catch {
      setUpdate({ state: 'idle' })
      toast('Keine Verbindung')
    }
  }

  return (
    <>
      <div className="about-head">
        <img src={appIcon} width={64} height={64} alt="" draggable={false} />
        <h3 className="about-name">Wolkenfunk</h3>
        {info && (
          <>
            <span className="about-version">Version {info.version}</span>
            <span className="muted">Kern {info.core}</span>
          </>
        )}
      </div>
      {info?.canUpdate && (
        <section className="settings-section" aria-live="polite">
          {update.state === 'ready' ? (
            <div className="settings-row">
              <span className="settings-text">
                <span>Wolkenfunk {update.version} ist bereit</span>
                <span className="person-sub wrap">Das Update wird beim nächsten Beenden installiert.</span>
              </span>
            </div>
          ) : (
            <button className="settings-row center-row" disabled={update.state === 'checking'} onClick={() => void check()}>
              {update.state === 'checking' ? 'Suche …' : update.state === 'current' ? 'Wolkenfunk ist aktuell' : 'Nach Updates suchen'}
            </button>
          )}
        </section>
      )}
      <section className="settings-section">
        <a className="settings-row" href="https://wolkenfunk.org" target="_blank" rel="noreferrer">
          <span className="settings-text">Webseite</span>
          <ExternalIcon />
        </a>
        <a className="settings-row" href="https://github.com/raph-ael/nextcloud-talk-legacy" target="_blank" rel="noreferrer">
          <span className="settings-text">Quellcode</span>
          <ExternalIcon />
        </a>
      </section>
      <p className="muted center about-license">Lizenz: AGPL-3.0</p>
    </>
  )
}

// --- icons (own drawings) ---------------------------------------------------------

function Icon({ children }: { children: ReactNode }) {
  return (
    <svg viewBox="0 0 24 24" width="20" height="20" aria-hidden fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
      {children}
    </svg>
  )
}
const GearIcon = () => (
  <Icon>
    <circle cx="12" cy="12" r="3" />
    <path d="M12 3.5v2.2M12 18.3v2.2M3.5 12h2.2M18.3 12h2.2M6 6l1.6 1.6M16.4 16.4L18 18M6 18l1.6-1.6M16.4 7.6L18 6" />
    <circle cx="12" cy="12" r="6.3" />
  </Icon>
)
const BrushIcon = () => (
  <Icon>
    <path d="M19.5 4.5l-8 8M9.5 12.5c-2 0-3.5 1.5-3.5 3.5 0 1.2-.8 2.3-2 2.5 1 1 2.4 1.5 4 1.5 2.5 0 4.5-2 4.5-4.5z" />
  </Icon>
)
const ChatIcon = () => (
  <Icon>
    <path d="M4.5 5h15v11h-9l-4.5 3.5V16h-1.5z" />
  </Icon>
)
const InfoIcon = () => (
  <Icon>
    <circle cx="12" cy="12" r="8.5" />
    <path d="M12 11v5.5M12 7.6v.1" />
  </Icon>
)
const ExternalIcon = () => (
  <span className="settings-icon">
    <Icon>
      <path d="M13.5 5.5h5v5M18.5 5.5L11 13M16 14v4.5H5.5V8H10" />
    </Icon>
  </span>
)
function Chevron() {
  return (
    <svg className="settings-chevron" viewBox="0 0 16 16" width="14" height="14" aria-hidden>
      <path fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" d="M6 3.5l4.5 4.5L6 12.5" />
    </svg>
  )
}
