// The own avatar at the top of the chat list and the account menu (docs/accounts.md): with
// one account it opens the settings, from two on the menu with every account, „Konto
// hinzufügen“ and „Einstellungen“.

import { useRef, useState, type ReactNode } from 'react'
import type { AccountRow } from '@shared/core'
import { MAX_ACCOUNTS, openSettings, startAddAccount, switchAccount, useStore } from '../store'
import { toast, useCall } from '../call/store'
import { Avatar } from './Avatar'
import { MessageMenu, type MenuItem } from './MessageMenu'
import { GearIcon } from './Settings'

const MOD = navigator.platform.startsWith('Mac') ? '⌘' : 'Strg+'
const LOCKED = 'Während eines Anrufs nicht möglich'

/** „99+“ above 99. */
export function countLabel(n: number): string {
  return n > 99 ? '99+' : String(n)
}

function chatsWithNews(n: number): string {
  return n === 1 ? '1 Chat mit Neuem' : `${n} Chats mit Neuem`
}

/** A call that is not ringing any more blocks switching to another account. */
export function useSwitchLocked(): boolean {
  return useCall((s) => !!s.call && s.call.state !== 'ended' && s.call.state !== 'incoming')
}

/** Accessible name of an account row (menu and settings). */
export function accountRowLabel(a: AccountRow, active: boolean, locked: boolean): string {
  return [
    `${a.name}, ${a.server}`,
    active ? 'aktiv' : a.chats > 0 ? chatsWithNews(a.chats) : '',
    locked && !active ? 'nicht verfügbar' : '',
  ]
    .filter(Boolean)
    .join(', ')
}

/** Click on an account row: switch, or say why not. */
export function chooseAccount(a: AccountRow, active: boolean, locked: boolean): void {
  if (active) return
  if (locked) toast(LOCKED)
  else void switchAccount(a.id)
}

export function OwnAvatar() {
  const account = useStore((s) => s.account)
  const accounts = useStore((s) => s.accounts)
  const locked = useSwitchLocked()
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null)
  const button = useRef<HTMLButtonElement>(null)
  /** A press on the avatar closes an open menu on mousedown already: its click must not reopen it. */
  const closedAt = useRef(0)
  const several = accounts.length >= 2
  const others = accounts.filter((a) => a.id !== account?.id).reduce((sum, a) => sum + a.chats, 0)

  if (!several) {
    return (
      <button className="own-avatar" aria-label="Einstellungen" title={`Einstellungen (${MOD},)`} onClick={() => openSettings()}>
        <Avatar name={account?.name ?? '?'} userId={account?.userId} size={28} />
      </button>
    )
  }

  const items: MenuItem[] = [
    ...accounts.map((a): MenuItem => {
      const active = a.id === account?.id
      return {
        label: a.name,
        detail: a.server,
        icon: <Avatar name={a.name} userId={a.userId} account={a.id} size={36} />,
        trailing: active ? <CheckIcon /> : a.chats > 0 ? <span className="badge">{countLabel(a.chats)}</span> : null,
        ariaLabel: accountRowLabel(a, active, locked),
        dimmed: locked && !active,
        onSelect: () => chooseAccount(a, active, locked),
      }
    }),
    { kind: 'separator' },
    ...(accounts.length < MAX_ACCOUNTS ? [{ label: 'Konto hinzufügen', icon: <PlusIcon />, onSelect: startAddAccount }] : []),
    { label: 'Einstellungen', icon: <GearIcon />, onSelect: () => openSettings() },
  ]

  function open() {
    const r = button.current?.getBoundingClientRect()
    if (r) setMenu({ x: r.left, y: r.bottom + 6 })
  }

  function close() {
    closedAt.current = performance.now()
    setMenu(null)
    // The focus goes back to the avatar (keyboard), once the menu is gone.
    requestAnimationFrame(() => {
      const b = button.current
      if (b?.isConnected && (document.activeElement === document.body || document.activeElement === null)) b.focus()
    })
  }

  return (
    <>
      <button
        ref={button}
        className="own-avatar"
        aria-label={others > 0 ? `Konten, Neues in ${others === 1 ? '1 Chat' : `${others} Chats`}` : 'Konten'}
        aria-haspopup="menu"
        aria-expanded={!!menu}
        title="Konten"
        onClick={() => {
          if (menu) close()
          else if (performance.now() - closedAt.current > 300) open()
        }}
      >
        <Avatar name={account?.name ?? '?'} userId={account?.userId} size={28} />
        {others > 0 && (
          <span className="avatar-badge" aria-hidden>
            {countLabel(others)}
          </span>
        )}
      </button>
      {menu && (
        <MessageMenu x={menu.x} y={menu.y} items={items} label="Konten" className="account-menu" onClose={close} />
      )}
    </>
  )
}

function Svg({ children }: { children: ReactNode }) {
  return (
    <svg
      viewBox="0 0 24 24"
      width="20"
      height="20"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.8"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      {children}
    </svg>
  )
}

export function CheckIcon() {
  return (
    <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden>
      <path fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" d="M3.5 8.5l3 3 6-7" />
    </svg>
  )
}

export function PlusIcon() {
  return (
    <Svg>
      <path d="M12 5v14M5 12h14" />
    </Svg>
  )
}
