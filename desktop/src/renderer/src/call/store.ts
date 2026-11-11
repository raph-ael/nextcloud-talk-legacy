// What the call UI shows: the core's CallView (docs/calls.md), plus how it is presented
// (full view or minimized behind the "back to call" bar) and the tones per state.

import { useSyncExternalStore } from 'react'
import type { CallView, EndReason } from '@shared/core'
import { playConnected, playHangup, playRecordingCue, playRingback, playRingtone, stopTones } from './tones'
import { startRecordingNotices } from './recordings'

/** How long the final state stays visible before the call view closes. */
const ENDED_MS = 1500

export interface CallUiState {
  call: CallView | null
  /** The call view is hidden behind the "back to call" bar (chats stay usable). */
  minimized: boolean
  /** Short message (failed action, „Dieser Anruf wird aufgezeichnet“); shown briefly. */
  toast: string | null
}

let st: CallUiState = { call: null, minimized: false, toast: null }
const listeners = new Set<() => void>()
let endTimer: ReturnType<typeof setTimeout> | null = null
let toastTimer: ReturnType<typeof setTimeout> | null = null
/** Call ids that already played the connected tone (a reconnect does not play it again). */
const greeted = new Set<string>()
/** Call whose running recording was announced (again after the recording stopped). */
let announcedRecording: string | null = null

function set(patch: Partial<CallUiState>): void {
  st = { ...st, ...patch }
  listeners.forEach((l) => l())
}

function subscribe(l: () => void): () => void {
  listeners.add(l)
  return () => listeners.delete(l)
}

export function useCall<T>(selector: (s: CallUiState) => T): T {
  return useSyncExternalStore(subscribe, () => selector(st))
}

export function callState(): CallUiState {
  return st
}

let started = false

export function startCallUi(): void {
  if (started) return
  started = true
  window.talk.onEvent((e) => {
    if (e.type === 'callChanged') show(e.call)
    else if (e.type === 'session' && e.state === 'loggedOut') show(null)
  })
  // After a reload the core still knows the call.
  void window.talk.currentCall().then((c) => {
    if (c && !st.call) show(c)
  })
  window.addEventListener('keydown', onKey, true)
  startRecordingNotices()
}

function show(next: CallView | null): void {
  const prev = st.call
  if (!next) {
    // `ended` stays on screen for a moment; without one (logout) the view closes now.
    if (prev && prev.state !== 'ended') close()
    return
  }
  if (endTimer && next.id !== prev?.id) {
    clearTimeout(endTimer)
    endTimer = null
  }
  tones(prev, next)
  announceRecording(next)
  if (next.state === 'ended' && quiet(next)) {
    // Not even a frame of the call view (an incoming bar just goes away).
    close()
    return
  }
  // A new call always opens the view; an incoming one shows as the bar on top.
  const minimized = next.id === prev?.id ? st.minimized : false
  set({ call: next, minimized })
  if (next.state === 'ended') {
    if (endTimer) clearTimeout(endTimer)
    endTimer = setTimeout(close, ENDED_MS)
  }
}

function close(): void {
  if (endTimer) clearTimeout(endTimer)
  endTimer = null
  stopTones()
  set({ call: null, minimized: false })
}

function tones(prev: CallView | null, next: CallView): void {
  const was = prev?.id === next.id ? prev.state : null
  if (was === next.state) return
  switch (next.state) {
    case 'incoming':
      playRingtone()
      break
    case 'ringing':
      // Groups wait silently ("Warte auf andere …").
      if (!next.isGroup && next.direction === 'outgoing') playRingback()
      else stopTones()
      break
    case 'connected':
      if (!greeted.has(next.id)) {
        greeted.add(next.id)
        playConnected()
      } else stopTones()
      break
    case 'ended':
      if (was && !quiet(next)) playHangup()
      else stopTones()
      break
    default:
      stopTones()
  }
}

/**
 * Ends without the hang-up tone and without lingering on screen: answered on another device,
 * and an incoming call we declined or missed (the bar just goes away; a missed call gets
 * its notification). The caller side of `declined` ("Abgelehnt") is a normal end.
 */
function quiet(c: CallView): boolean {
  const kind = c.endReason?.kind
  if (kind === 'answeredElsewhere') return true
  return c.direction === 'incoming' && (kind === 'declined' || kind === 'missed')
}

export function isRecording(c: CallView): boolean {
  return c.recording === 'audio' || c.recording === 'video'
}

/**
 * Once the recording runs (also when we join a running one): toast, a soft cue and, through
 * the toast's live region, a screen reader announcement. Not again after a reconnect.
 */
function announceRecording(c: CallView): void {
  if (c.state === 'ended') return
  const joined = c.state === 'connected' || c.state === 'reconnecting'
  if (isRecording(c)) {
    if (!joined || announcedRecording === c.id) return
    announcedRecording = c.id
    playRecordingCue()
    toast('Dieser Anruf wird aufgezeichnet')
  } else if (announcedRecording === c.id && c.state === 'connected') {
    // Stopped (or failed): a new recording is announced again.
    announcedRecording = null
  }
}

// --- Actions ----------------------------------------------------------------------------

export function toast(text: string): void {
  if (toastTimer) clearTimeout(toastTimer)
  set({ toast: text })
  toastTimer = setTimeout(() => set({ toast: null }), 4000)
}

/** The core's message without the IPC and error-code prefixes. */
export function errorText(err: unknown): string {
  const msg = String((err as Error)?.message ?? err)
    .replace(/^Error invoking remote method '[^']*': /, '')
    .replace(/^Error: /, '')
  if (msg.startsWith('NETWORK:')) return 'Keine Verbindung zum Server'
  return msg.replace(/^[A-Z_]+: /, '').replace(/^call: [\w-]+: /, '')
}

function fail(err: unknown): void {
  const msg = errorText(err)
  toast(msg.includes('busy') ? 'Es läuft bereits ein Anruf.' : `Anruf nicht möglich: ${msg}`)
}

export function startCall(token: string): void {
  if (st.call && st.call.state !== 'ended') {
    if (st.call.token === token) set({ minimized: false })
    else fail(new Error('busy'))
    return
  }
  window.talk.startCall(token, false).catch(fail)
}

export function accept(): void {
  const c = st.call
  if (c?.state !== 'incoming') return
  stopTones()
  window.talk.acceptCall(c.token).catch(fail)
}

export function decline(): void {
  const c = st.call
  if (c?.state !== 'incoming') return
  stopTones()
  void window.talk.declineCall(c.token)
}

export function hangUp(): void {
  if (!st.call || st.call.state === 'ended') return
  void window.talk.hangUp()
}

export function toggleMute(): void {
  const c = st.call
  if (!c || c.state === 'ended' || c.state === 'incoming') return
  void window.talk.setCallMuted(!c.muted)
}

/** Only with `canRecord`; the state comes back through `callChanged`. */
export function startRecording(video: boolean): void {
  window.talk.startRecording(video).catch((e) => toast(errorText(e)))
}

export function stopRecording(): void {
  window.talk.stopRecording().catch((e) => toast(errorText(e)))
}

/** „Zustimmen und beitreten“ (`recordingConsentPending`); „Abbrechen“ is `hangUp`. */
export function consentToRecording(): void {
  void window.talk.consentToRecording()
}

export function minimize(minimized: boolean): void {
  if (st.call) set({ minimized })
}

/** Like Signal Desktop: Ctrl+Shift+A accept, Ctrl+Shift+D decline/hang up, Ctrl+Shift+M mute. */
function onKey(e: KeyboardEvent): void {
  if (!(e.ctrlKey || e.metaKey) || !e.shiftKey || e.altKey) return
  const c = st.call
  if (!c) return
  const key = e.key.toLowerCase()
  if (key === 'a' && c.state === 'incoming') accept()
  else if (key === 'd') {
    if (c.state === 'incoming') decline()
    else hangUp()
  } else if (key === 'm') toggleMute()
  else return
  e.preventDefault()
  e.stopPropagation()
}

// --- Texts (docs/calls.md) --------------------------------------------------------------

/** Status line under the name; `null` while connected (the timer shows instead). */
export function statusText(c: CallView): string | null {
  switch (c.state) {
    case 'dialing':
      return 'Anrufen …'
    case 'ringing':
      return c.isGroup ? 'Warte auf andere …' : 'Es klingelt …'
    case 'incoming':
      return c.isGroup ? 'Gruppenanruf' : 'Talk-Anruf'
    case 'connecting':
      return 'Verbindung wird hergestellt …'
    case 'connected':
      return null
    case 'reconnecting':
      return 'Verbindung wird neu hergestellt …'
    case 'ended':
      return endText(c.endReason)
  }
}

function endText(r: EndReason | null): string {
  switch (r?.kind) {
    case 'noAnswer':
      return 'Keine Antwort'
    case 'missed':
      return 'Verpasster Anruf'
    case 'declined':
      return 'Abgelehnt'
    case 'answeredElsewhere':
      return 'Auf einem anderen Gerät angenommen'
    case 'failed':
      return r.message || 'Anruf fehlgeschlagen'
    default:
      return 'Anruf beendet'
  }
}

/** m:ss, from one hour h:mm:ss. */
export function duration(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000))
  const h = Math.floor(s / 3600)
  const m = Math.floor((s % 3600) / 60)
  const sec = String(s % 60).padStart(2, '0')
  return h > 0 ? `${h}:${String(m).padStart(2, '0')}:${sec}` : `${m}:${sec}`
}
