// „Darstellung“ (docs/settings.md): the main process keeps theme and accent and sets
// `nativeTheme.themeSource`, so `prefers-color-scheme` already follows the theme. Here the
// accent goes onto :root as --accent, --accent-hover and --bubble-own for the current mode,
// and the view as `data-layout` (docs/compact-view.md). „Gruppen und Direktchats trennen“
// lives here too; the store asks the core for the sections again when it changes.

import { useSyncExternalStore } from 'react'
import type { Appearance, Layout, Theme } from '@shared/core'
import { accentById } from './accents'

const darkQuery = window.matchMedia('(prefers-color-scheme: dark)')
let current: Appearance = { theme: 'system', accent: 'blau', layout: 'bubbles', splitChats: true }
let dark = darkQuery.matches
const listeners = new Set<() => void>()

function apply(): void {
  const colors = dark ? accentById(current.accent).dark : accentById(current.accent).light
  const root = document.documentElement.style
  root.setProperty('--accent', colors.accent)
  root.setProperty('--accent-hover', colors.hover)
  root.setProperty('--bubble-own', colors.bubble)
  document.documentElement.dataset.layout = current.layout
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
    current = { theme: e.theme, accent: e.accent, layout: e.layout, splitChats: e.splitChats }
    apply()
  })
  current = await window.talk.appearance().catch(() => current)
  apply()
}

/** Applies at once here; the main process saves it and switches the theme. */
export function setAppearance(theme: Theme, accent: string): void {
  current = { ...current, theme, accent }
  apply()
  void window.talk.setAppearance(theme, accent)
}

export function setLayout(layout: Layout): void {
  current = { ...current, layout }
  apply()
  void window.talk.setLayout(layout)
}

export function setSplitChats(on: boolean): void {
  current = { ...current, splitChats: on }
  apply()
  void window.talk.setSplitChats(on)
}

/** The current setting, outside React (the store). */
export function splitChats(): boolean {
  return current.splitChats
}

/** The compact view is on (docs/compact-view.md), outside React. */
export function isCompact(): boolean {
  return current.layout === 'compact'
}

/** Called after every change of the appearance (also dark mode); returns the unsubscribe. */
export function onAppearance(listener: () => void): () => void {
  listeners.add(listener)
  return () => listeners.delete(listener)
}

let snapshot = { ...current, dark }

/** Theme, accent, view, split and whether the window shows dark right now. */
export function useAppearance(): Appearance & { dark: boolean } {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l)
      return () => listeners.delete(l)
    },
    () => {
      if (
        snapshot.theme !== current.theme ||
        snapshot.accent !== current.accent ||
        snapshot.layout !== current.layout ||
        snapshot.splitChats !== current.splitChats ||
        snapshot.dark !== dark
      )
        snapshot = { ...current, dark }
      return snapshot
    },
  )
}
