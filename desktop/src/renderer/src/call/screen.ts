// Own screen share (docs/calls.md „Bildschirmfreigabe“): picking (own dialog with thumbnails,
// or the system's portal on Linux/Wayland), the captured track for the screenPublisher peers
// (engine.ts), and its end. Only the picture, no system sound.

import { useSyncExternalStore } from 'react'
import type { ScreenSource } from '@shared/core'

/** docs/calls.md „Qualität“: at most 1920×1080, 15 fps (at most 30). */
const CONSTRAINTS: DisplayMediaStreamOptions = {
  video: { width: { max: 1920 }, height: { max: 1080 }, frameRate: { ideal: 15, max: 30 } },
  audio: false,
}

export interface ScreenState {
  /** Our captured screen while we share (also the small „Dein Bildschirm“ tile). */
  stream: MediaStream | null
  /** The own picker is open, with what it shows (refreshed every 2 s). */
  picker: ScreenSource[] | null
  /** Between the pick and the running track (system dialog, getDisplayMedia). */
  starting: boolean
}

let st: ScreenState = { stream: null, picker: null, starting: false }
const listeners = new Set<() => void>()
let sink: (track: MediaStreamTrack | null) => void = () => undefined
let notice: (text: string) => void = () => undefined
/** Every start and stop counts up; an older start that finishes late lets its track go. */
let gen = 0

function set(patch: Partial<ScreenState>): void {
  st = { ...st, ...patch }
  listeners.forEach((l) => l())
}

export function useScreenShare(): ScreenState {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l)
      return () => listeners.delete(l)
    },
    () => st,
  )
}

export function screenState(): ScreenState {
  return st
}

/** The screen track for the screenPublisher peers (engine.ts), or none. */
export function currentScreen(): MediaStreamTrack | null {
  return st.stream?.getVideoTracks()[0] ?? null
}

/** engine.ts: puts a new screen track (or none) on every screenPublisher peer. */
export function setScreenSink(f: (track: MediaStreamTrack | null) => void): void {
  sink = f
}

/** call/store.ts: short messages in the call view. */
export function setScreenNotice(f: (text: string) => void): void {
  notice = f
}

/** „Bildschirm teilen“: the own picker, or on Linux/Wayland right away the portal. */
export async function openScreenShare(): Promise<void> {
  if (st.stream || st.picker || st.starting) return
  const my = ++gen
  set({ starting: true })
  let sources: ScreenSource[] | null
  try {
    sources = await window.talk.screenSources()
  } catch (err) {
    console.warn('screen sources:', (err as Error).message)
    set({ starting: false })
    notice('Bildschirm teilen ist gerade nicht möglich')
    return
  }
  if (my !== gen) return
  if (sources === null) return capture(null, my)
  set({ starting: false, picker: sources })
}

/** The picker shows fresh thumbnails while it is open. */
export async function refreshScreenSources(): Promise<void> {
  if (!st.picker) return
  const list = await window.talk.screenSources().catch(() => null)
  if (list && st.picker) set({ picker: list })
}

export function cancelScreenPicker(): void {
  if (st.picker) set({ picker: null })
}

/** „Teilen“ in the picker. */
export function shareScreen(sourceId: string): void {
  if (!st.picker) return
  set({ picker: null })
  void capture(sourceId, ++gen)
}

async function capture(sourceId: string | null, my: number): Promise<void> {
  set({ starting: true })
  let stream: MediaStream
  try {
    await window.talk.chooseScreen(sourceId)
    stream = await navigator.mediaDevices.getDisplayMedia(CONSTRAINTS)
  } catch (err) {
    if (my !== gen) return
    set({ starting: false })
    const name = (err as DOMException)?.name
    // Cancelled in the system dialog: nothing to say.
    if (sourceId === null && name === 'NotAllowedError') return
    console.warn('getDisplayMedia:', name, (err as Error).message)
    notice(sourceId === null ? 'Bildschirm teilen ist gerade nicht möglich' : 'Dieses Fenster lässt sich nicht teilen')
    return
  }
  const track = stream.getVideoTracks()[0]
  if (my !== gen || !track) {
    // Stopped (or the call ended) while the system dialog was open.
    stream.getTracks().forEach((t) => t.stop())
    if (my === gen) set({ starting: false })
    return
  }
  track.contentHint = 'detail'
  // Some capturers ignore the getDisplayMedia limits; the track takes them.
  await track.applyConstraints(CONSTRAINTS.video as MediaTrackConstraints).catch(() => undefined)
  // „Freigabe beenden“ of the system, the shared window closed, sleep.
  track.addEventListener('ended', () => {
    if (currentScreen() !== track) return
    gen++
    clear()
    window.talk.mediaReport({ type: 'screenState', on: false })
    notice('Bildschirmfreigabe beendet')
  })
  set({ stream, starting: false })
  sink(track)
  // The core now creates the screenPublisher peers; each gets this track (engine.ts).
  void window.talk.setScreenShare(true)
}

/** „Teilen beenden“; the core answers with stopScreen (stopScreenTrack). */
export function stopScreenShare(): void {
  const sharing = !!st.stream
  stopScreenTrack()
  if (sharing) void window.talk.setScreenShare(false)
}

/** `stopScreen` from the core, or the call is over: the track ends, the system indicator goes. */
export function stopScreenTrack(): void {
  gen++
  if (st.picker || st.starting || st.stream) clear()
}

function clear(): void {
  st.stream?.getTracks().forEach((t) => t.stop())
  set({ stream: null, picker: null, starting: false })
  sink(null)
}

/** Diagnostics for tests (engine snapshot). */
export function screenSnapshot(): unknown {
  const t = currentScreen()
  return t ? { label: t.label, settings: t.getSettings(), contentHint: t.contentHint, state: t.readyState } : null
}
