// „Darstellung“ (docs/settings.md): the main process keeps theme and accent and sets
// `nativeTheme.themeSource`, so `prefers-color-scheme` already follows the theme. Here the
// accent goes onto :root as --accent, --accent-hover and --bubble-own for the current mode.

import { useSyncExternalStore } from 'react'
import type { Appearance, Theme } from '@shared/core'
import { accentById } from './accents'

const darkQuery = window.matchMedia('(prefers-color-scheme: dark)')
let current: Appearance = { theme: 'system', accent: 'blau' }
let dark = darkQuery.matches
const listeners = new Set<() => void>()

function apply(): void {
  const colors = dark ? accentById(current.accent).dark : accentById(current.accent).light
  const root = document.documentElement.style
  root.setProperty('--accent', colors.accent)
  root.setProperty('--accent-hover', colors.hover)
  root.setProperty('--bubble-own', colors.bubble)
  listeners.forEach((l) => l())
}

/** Before the first render (main.tsx): nothing shows in the default accent first. */
export async function loadAppearance(): Promise<void> {
  darkQuery.addEventListener('change', (e) => {
    dark = e.matches
    apply()
  })
  window.talk.onEvent((e) => {
    if (e.type !== 'appearanceChanged') return
    current = { theme: e.theme, accent: e.accent }
    apply()
  })
  current = await window.talk.appearance().catch(() => current)
  apply()
}

/** Applies at once here; the main process saves it and switches the theme. */
export function setAppearance(theme: Theme, accent: string): void {
  current = { theme, accent }
  apply()
  void window.talk.setAppearance(theme, accent)
}

let snapshot = { ...current, dark }

/** Theme, accent and whether the window shows dark right now. */
export function useAppearance(): Appearance & { dark: boolean } {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l)
      return () => listeners.delete(l)
    },
    () => {
      if (snapshot.theme !== current.theme || snapshot.accent !== current.accent || snapshot.dark !== dark)
        snapshot = { ...current, dark }
      return snapshot
    },
  )
}
