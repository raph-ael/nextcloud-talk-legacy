// „Einstellungen“ (docs/settings.md): replace the whole window in two columns – on the left
// profile and rows in the width of the chat list, on the right the chosen page. The chat
// list and an open chat stay mounted underneath (draft, scroll position); a call goes on.

import { useEffect, useRef, useState, type ReactNode } from 'react'
import type { AppInfo, Layout, Theme, UpdateCheck } from '@shared/core'
import { closeSettings, MAX_ACCOUNTS, setSettingsPage, startAddAccount, useStore, type SettingsPage } from '../store'
import { toast } from '../call/store'
import { ACCENTS } from '../accents'
import { setAppearance, setLayout, setSplitChats, useAppearance } from '../appearance'
import { Avatar } from './Avatar'
import { Categories } from './Categories'
import { ConfirmDialog } from './Modal'
import { accountRowLabel, chooseAccount, countLabel, PlusIcon, useSwitchLocked } from './AccountMenu'
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

/** „Ansicht“ (docs/compact-view.md). */
const LAYOUTS: { layout: Layout; label: string; sub?: string }[] = [
  { layout: 'bubbles', label: 'Blasen' },
  { layout: 'compact', label: 'Kompakt', sub: 'Einzeilige Chatliste, Nachrichten untereinander' },
]

/** The preview of „Darstellung“ in the compact view: two runs with their heads. */
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

export function Settings() {
  const page = useStore((s) => s.settings) ?? 'appearance'
  const account = useStore((s) => s.account)
  const [chats, setChats] = useState(false)
  const [confirmLogout, setConfirmLogout] = useState(false)
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
    { page: 'general', label: 'Allgemein', icon: <GearIcon /> },
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
            <Avatar name={account?.name ?? '?'} userId={account?.userId} size={64} />
            <span className="settings-profile-name">{account?.name}</span>
            <span className="muted">{account?.server.replace(/^https?:\/\//, '')}</span>
          </div>
          <OtherAccounts />
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
            <button className="settings-nav-row danger" onClick={() => setConfirmLogout(true)}>
              <span className="settings-text">Abmelden</span>
            </button>
          </div>
        </div>
      </aside>
      {confirmLogout && account && <LogoutDialog name={account.name} server={account.server} onClose={() => setConfirmLogout(false)} />}
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

/** „Konten“ (docs/accounts.md): the other accounts (a click switches, the settings stay), „Konto hinzufügen“. */
function OtherAccounts() {
  const active = useStore((s) => s.account?.id)
  const accounts = useStore((s) => s.accounts)
  const locked = useSwitchLocked()
  const others = accounts.filter((a) => a.id !== active)
  return (
    <section className="settings-accounts" aria-labelledby="settings-accounts">
      <h3 className="settings-heading" id="settings-accounts">
        Konten
      </h3>
      {others.map((a) => (
        <button
          key={a.id}
          className={`settings-nav-row account-row${locked ? ' dimmed' : ''}`}
          aria-label={accountRowLabel(a, false, locked)}
          onClick={() => chooseAccount(a, false, locked)}
        >
          <Avatar name={a.name} userId={a.userId} account={a.id} size={36} />
          <span className="settings-text">
            <span className="account-row-name">{a.name}</span>
            <span className="account-row-server">{a.server}</span>
          </span>
          {a.chats > 0 && (
            <span className="badge" aria-hidden>
              {countLabel(a.chats)}
            </span>
          )}
        </button>
      ))}
      {accounts.length < MAX_ACCOUNTS && (
        <button className="settings-nav-row" onClick={startAddAccount}>
          <span className="settings-icon">
            <PlusIcon />
          </span>
          <span className="settings-text">Konto hinzufügen</span>
        </button>
      )}
    </section>
  )
}

/** „Abmelden?“ for the shown account only; the others go on. */
function LogoutDialog({ name, server, onClose }: { name: string; server: string; onClose: () => void }) {
  const [busy, setBusy] = useState(false)
  return (
    <ConfirmDialog
      title="Abmelden?"
      text={`${name} (${server.replace(/^https?:\/\//, '')}) wird auf diesem Gerät abgemeldet. Ungesendete Nachrichten und der Offline-Speicher dieses Kontos werden gelöscht.`}
      confirm="Abmelden"
      danger
      busy={busy}
      onCancel={onClose}
      onConfirm={() => {
        setBusy(true)
        void window.talk
          .logout()
          .catch((e: Error) => toast(`Abmelden fehlgeschlagen: ${e.message}`))
          .finally(onClose)
      }}
    />
  )
}

/**
 * „Allgemein“: „Beim Schließen im Hintergrund weiterlaufen“ (Linux, Windows; the same setting
 * as the check mark in the tray menu) and „Beim Anmelden starten“ (not in a Flatpak).
 */
function GeneralPage() {
  const [on, setOn] = useState<boolean | null>(null)
  /** `undefined`: not known yet, `null`: no such switch here. */
  const [atLogin, setAtLogin] = useState<boolean | null | undefined>(undefined)
  useEffect(() => {
    let alive = true
    void window.talk.closeToTray().then((v) => alive && setOn(v))
    void window.talk.openAtLogin().then((v) => alive && setAtLogin(v))
    const off = window.talk.onEvent((e) => e.type === 'closeToTrayChanged' && setOn(e.on))
    return () => {
      alive = false
      off()
    }
  }, [])
  return (
    <section className="settings-section">
      {!IS_MAC && (
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
      )}
      {atLogin !== null && (
        <button
          className="settings-row"
          role="switch"
          aria-checked={!!atLogin}
          disabled={atLogin === undefined}
          onClick={() => {
            setAtLogin(!atLogin)
            void window.talk.setOpenAtLogin(!atLogin)
          }}
        >
          <span className="settings-text">
            <span>Beim Anmelden starten</span>
            <span className="person-sub wrap">Wolkenfunk startet im Hintergrund, damit Nachrichten und Anrufe ankommen</span>
          </span>
          <span className={`switch${atLogin ? ' on' : ''}`} aria-hidden />
        </button>
      )}
    </section>
  )
}

/** „Darstellung“: preview, design, accent, view and chat list; every change applies at once. */
function AppearancePage() {
  const { theme, accent, dark, layout, splitChats } = useAppearance()
  return (
    <>
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
      <section className="settings-section">
        <h3 className="settings-heading" id="settings-layout">
          Ansicht
        </h3>
        <div className="choice-list settings-choices" role="radiogroup" aria-labelledby="settings-layout">
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
      </section>
      <section className="settings-section">
        <h3 className="settings-heading">Chatliste</h3>
        <button className="settings-row" role="switch" aria-checked={splitChats} onClick={() => setSplitChats(!splitChats)}>
          <span className="settings-text">
            <span>Gruppen und Direktchats trennen</span>
            <span className="person-sub wrap">Eigene Abschnitte statt „Chats“</span>
          </span>
          <span className={`switch${splitChats ? ' on' : ''}`} aria-hidden />
        </button>
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
export const GearIcon = () => (
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
