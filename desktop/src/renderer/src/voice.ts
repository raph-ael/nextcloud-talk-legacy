// Voice messages in the renderer: microphone capture (PCM to the core, which writes MP3 to
// disk as it goes), drafts per conversation, and one shared audio player.
//
// Like Signal Desktop: the recording stops and stays as draft when the window loses
// focus, the chat changes or one hour is reached; Esc cancels.

import { useSyncExternalStore } from 'react'
import { VOICE_MAX_MS, type AudioInfo, type VoiceDraft } from '@shared/core'
import workletUrl from './voice-worklet.js?url'
import { accountId, getState, showQueued, toReply, type UiMessage } from './store'
import { openMicrophone } from './call/devices'

/** Show a hint this long before the one-hour limit. */
const WARN_BEFORE_MS = 5 * 60 * 1000
/** Live level bars kept for the recording bar. */
const LEVEL_BARS = 48
/** Received voice messages up to this size are fetched right away (for the waveform). */
const AUTO_FETCH_BYTES = 15 * 1024 * 1024
export const RATES = [1, 1.5, 2, 0.5] as const

export interface Recording {
  /** The account it records for (docs/accounts.md): its draft stays there. */
  account: string
  token: string
  durationMs: number
  /** Last levels 0..1, oldest first. */
  levels: number[]
  /** Approaching the one-hour limit. */
  warn: boolean
  starting: boolean
}

interface VoiceState {
  recording: Recording | null
  /** Draft per account and conversation (`draftKey`): undefined = not loaded yet. */
  drafts: Record<string, VoiceDraft | null>
  /** Why the last recording stopped or failed (shown once in the composer); `key`: `draftKey`. */
  notice: { key: string; text: string } | null
}

let vs: VoiceState = { recording: null, drafts: {}, notice: null }
const listeners = new Set<() => void>()

function set(patch: Partial<VoiceState>): void {
  vs = { ...vs, ...patch }
  listeners.forEach((l) => l())
}

function subscribe(l: () => void): () => void {
  listeners.add(l)
  return () => listeners.delete(l)
}

export function useVoice<T>(selector: (s: VoiceState) => T): T {
  return useSyncExternalStore(subscribe, () => selector(vs))
}

export function clearNotice(): void {
  if (vs.notice) set({ notice: null })
}

/** Tokens of different servers overlap: drafts go by account and token. */
export function draftKey(token: string, account = accountId()): string {
  return `${account}|${token}`
}

/** The recording of this chat in the account shown. */
export function recordingFor(s: VoiceState, token: string): Recording | null {
  return s.recording?.token === token && s.recording.account === accountId() ? s.recording : null
}

function setDraft(key: string, draft: VoiceDraft | null): void {
  set({ drafts: { ...vs.drafts, [key]: draft } })
}

// --- recording ------------------------------------------------------------------------

interface Capture {
  stream: MediaStream
  ctx: AudioContext
  node: AudioWorkletNode
  /** Pushes in order; awaited before finishing. */
  chain: Promise<void>
  stopped: boolean
}

let capture: Capture | null = null

/** Read through a function: TypeScript would keep a narrowing across awaits. */
const currentRecording = (): Recording | null => vs.recording

function micError(err: unknown): string {
  const name = (err as { name?: string })?.name
  if (name === 'NotAllowedError' || name === 'SecurityError') return 'Kein Zugriff auf das Mikrofon.'
  if (name === 'NotFoundError' || name === 'OverconstrainedError') return 'Kein Mikrofon gefunden.'
  if (name === 'NotReadableError') return 'Das Mikrofon wird gerade von einer anderen App benutzt.'
  return `Aufnahme nicht möglich: ${(err as Error)?.message ?? err}`
}

export async function startRecording(token: string): Promise<void> {
  if (vs.recording || capture) return
  const account = accountId()
  set({ recording: { account, token, durationMs: 0, levels: [], warn: false, starting: true }, notice: null })
  let stream: MediaStream | null = null
  let ctx: AudioContext | null = null
  try {
    // The microphone chosen for calls, through the device queue.
    stream = await openMicrophone({ channelCount: 1 })
    ctx = new AudioContext({ sampleRate: 48000 })
    await ctx.audioWorklet.addModule(workletUrl)
    await window.talk.voiceStart(token, ctx.sampleRate)
    const node = new AudioWorkletNode(ctx, 'pcm-capture', {
      numberOfInputs: 1,
      numberOfOutputs: 1,
      channelCount: 1,
      channelCountMode: 'explicit',
    })
    const c: Capture = { stream, ctx, node, chain: Promise.resolve(), stopped: false }
    node.port.onmessage = (e) => {
      if (e.data instanceof ArrayBuffer) {
        const pcm = new Uint8Array(e.data)
        c.chain = c.chain.then(() => push(c, token, pcm))
      }
    }
    ctx.createMediaStreamSource(stream).connect(node)
    // The node writes silence; connected so the graph keeps pulling it.
    node.connect(ctx.destination)
    capture = c
    stream.getAudioTracks()[0]?.addEventListener('ended', () => void stopToDraft('Das Mikrofon wurde getrennt.'))
    const r = currentRecording()
    if (r?.token === token) set({ recording: { ...r, starting: false } })
  } catch (err) {
    stream?.getTracks().forEach((t) => t.stop())
    void ctx?.close()
    await window.talk.voiceCancel().catch(() => undefined)
    set({ recording: null, notice: { key: draftKey(token, account), text: micError(err) } })
  }
}

async function push(c: Capture, token: string, pcm: Uint8Array): Promise<void> {
  if (c.stopped && pcm.byteLength === 0) return
  try {
    const status = await window.talk.voicePush(pcm)
    const r = vs.recording
    if (r && r.token === token && capture === c) {
      const levels = [...r.levels, status.level].slice(-LEVEL_BARS)
      set({
        recording: { ...r, durationMs: status.durationMs, levels, warn: status.durationMs >= VOICE_MAX_MS - WARN_BEFORE_MS },
      })
    }
    if (status.limitReached && !c.stopped) {
      void stopToDraft('Sprachnachrichten sind auf 60 Minuten begrenzt. Die Aufnahme liegt als Entwurf bereit.')
    }
  } catch (err) {
    if (!c.stopped) void stopToDraft(`Aufnahme gestoppt: ${(err as Error).message}`)
  }
}

/** Stops the microphone after handing the last samples to the core. */
async function stopCapture(): Promise<void> {
  const c = capture
  capture = null
  if (!c) return
  if (!c.stopped) {
    await new Promise<void>((resolve) => {
      const done = (e: MessageEvent) => {
        if (e.data === 'flushed') resolve()
      }
      c.node.port.addEventListener('message', done)
      c.node.port.postMessage('flush')
      setTimeout(resolve, 300) // context already suspended
    })
  }
  c.stopped = true
  c.stream.getTracks().forEach((t) => t.stop())
  void c.ctx.close()
  await c.chain
}

/** Ends the recording as the chat's draft (blur, chat switch, limit, explicit stop). */
export async function stopToDraft(notice?: string): Promise<VoiceDraft | null> {
  const r = vs.recording
  if (!r) return null
  await stopCapture()
  const draft = await window.talk.voiceFinish().catch(() => null)
  const key = draftKey(r.token, r.account)
  set({ recording: null, notice: notice && draft ? { key, text: notice } : vs.notice })
  setDraft(key, draft)
  return draft
}

export async function cancelRecording(): Promise<void> {
  const r = vs.recording
  if (!r) return
  await stopCapture()
  await window.talk.voiceCancel().catch(() => undefined)
  set({ recording: null })
}

/** Stops and sends right away (the send button of the recording bar). */
export async function sendRecording(replyTo: UiMessage | null): Promise<void> {
  const r = vs.recording
  if (!r) return
  const draft = await stopToDraft()
  if (r.account !== accountId()) return
  if (draft) await sendDraft(r.token, replyTo)
  else set({ notice: { key: draftKey(r.token, r.account), text: 'Zu kurz – zum Aufnehmen etwas länger sprechen.' } })
}

export async function loadDraft(token: string): Promise<void> {
  if (recordingFor(vs, token)) return
  const key = draftKey(token)
  // An answer after a switch never arrives (dropped): nothing lands in the other account.
  setDraft(key, await window.talk.voiceDraft(token).catch(() => null))
}

export async function discardDraft(token: string): Promise<void> {
  const key = draftKey(token)
  stopPlayback(`draft:${vs.drafts[key]?.id}`)
  setDraft(key, null)
  await window.talk.voiceDiscard(token)
}

export async function sendDraft(token: string, replyTo: UiMessage | null): Promise<void> {
  const key = draftKey(token)
  const draft = vs.drafts[key]
  if (draft) stopPlayback(`draft:${draft.id}`)
  try {
    const p = await window.talk.voiceSend(token, toReply(replyTo))
    setDraft(key, null)
    showQueued(token, p)
  } catch (err) {
    set({ notice: { key, text: `Senden nicht möglich: ${(err as Error).message}` } })
  }
}

// Like Signal Desktop: leaving the window keeps what was recorded as draft.
window.addEventListener('blur', () => {
  if (vs.recording && !vs.recording.starting) void stopToDraft()
})
document.addEventListener('visibilitychange', () => {
  if (document.visibilityState === 'hidden' && vs.recording) void stopToDraft()
})

// --- playback -------------------------------------------------------------------------

/** Something playable: a message's attachment or a draft. */
export interface Source {
  key: string
  /** Resolves the local file (downloads received files once). */
  resolve: () => Promise<string>
  /** For autoplay of the following voice message. */
  account?: string
  token?: string
  messageId?: number
}

export interface PlayerState {
  key: string | null
  playing: boolean
  positionMs: number
  durationMs: number
  loading: boolean
  rate: number
  error: string | null
}

function storedRate(): number {
  try {
    const r = Number(localStorage.getItem('voiceRate'))
    return (RATES as readonly number[]).includes(r) ? r : 1
  } catch {
    return 1
  }
}

const audio = new Audio()
audio.preload = 'auto'
let ps: PlayerState = { key: null, playing: false, positionMs: 0, durationMs: 0, loading: false, rate: storedRate(), error: null }
let current: Source | null = null
const playerListeners = new Set<() => void>()

function setPlayer(patch: Partial<PlayerState>): void {
  ps = { ...ps, ...patch }
  playerListeners.forEach((l) => l())
}

export function usePlayer(key: string): PlayerState | null {
  return useSyncExternalStore(
    (l) => {
      playerListeners.add(l)
      return () => playerListeners.delete(l)
    },
    () => (ps.key === key ? ps : null),
  )
}

export function useRate(): number {
  return useSyncExternalStore(
    (l) => {
      playerListeners.add(l)
      return () => playerListeners.delete(l)
    },
    () => ps.rate,
  )
}

audio.addEventListener('timeupdate', () => setPlayer({ positionMs: audio.currentTime * 1000 }))
audio.addEventListener('durationchange', () => {
  if (Number.isFinite(audio.duration)) setPlayer({ durationMs: audio.duration * 1000 })
})
audio.addEventListener('playing', () => setPlayer({ playing: true, loading: false }))
audio.addEventListener('pause', () => setPlayer({ playing: false }))
audio.addEventListener('waiting', () => setPlayer({ loading: true }))
audio.addEventListener('canplay', () => setPlayer({ loading: false }))
audio.addEventListener('error', () => setPlayer({ playing: false, loading: false, error: 'Wiedergabe nicht möglich' }))
audio.addEventListener('ended', () => {
  const done = current
  setPlayer({ playing: false, positionMs: 0 })
  audio.currentTime = 0
  if (done) playNextAfter(done)
})

export async function togglePlay(src: Source): Promise<void> {
  if (ps.key === src.key) {
    if (audio.paused) await audio.play().catch(() => undefined)
    else audio.pause()
    return
  }
  await playSource(src, 0)
}

async function playSource(src: Source, startFraction: number): Promise<void> {
  audio.pause()
  current = src
  setPlayer({ key: src.key, playing: false, positionMs: 0, durationMs: 0, loading: true, error: null })
  try {
    const path = await src.resolve()
    if (current !== src) return
    audio.src = window.talk.localFileUrl(path)
    audio.playbackRate = ps.rate
    audio.defaultPlaybackRate = ps.rate
    if (startFraction > 0) {
      await new Promise<void>((r) => audio.addEventListener('loadedmetadata', () => r(), { once: true }))
      audio.currentTime = startFraction * audio.duration
    }
    await audio.play()
  } catch (err) {
    if (current === src) setPlayer({ loading: false, error: `Wiedergabe nicht möglich: ${(err as Error).message}` })
  }
}

/** Jumps to `fraction` (0..1); starts playing a different source from there. */
export function seek(src: Source, fraction: number): void {
  const f = Math.min(1, Math.max(0, fraction))
  if (ps.key === src.key && Number.isFinite(audio.duration)) {
    audio.currentTime = f * audio.duration
    setPlayer({ positionMs: audio.currentTime * 1000 })
    if (audio.paused) void audio.play().catch(() => undefined)
  } else {
    void playSource(src, f)
  }
}

export function cycleRate(): void {
  const i = RATES.indexOf(ps.rate as (typeof RATES)[number])
  const rate = RATES[(i + 1) % RATES.length]
  audio.playbackRate = rate
  audio.defaultPlaybackRate = rate
  try {
    localStorage.setItem('voiceRate', String(rate))
  } catch {
    // per-viewer convenience only
  }
  setPlayer({ rate })
}

export function stopPlayback(key?: string): void {
  if (key && ps.key !== key) return
  audio.pause()
  audio.removeAttribute('src')
  current = null
  setPlayer({ key: null, playing: false, positionMs: 0 })
}

export function isAudioAttachment(m: UiMessage): boolean {
  const a = m.attachment
  return !!a && m.kind !== 'deleted' && (a.voiceMessage || a.mimetype.startsWith('audio/'))
}

/** Playable source of a message: the outbox copy while pending, else the downloaded file. */
export function messageSource(m: UiMessage): Source {
  const a = m.attachment!
  const local = m.localPath && (m.status || !a.id) ? m.localPath : null
  const account = accountId()
  return {
    // Message ids of different servers overlap.
    key: m.referenceId ? `ref:${m.referenceId}` : `${account}:id:${m.id}`,
    account,
    token: m.token,
    messageId: m.id,
    resolve: () => (local ? Promise.resolve(local) : fetchCached(a.path, a.size)),
  }
}

export function draftSource(d: VoiceDraft): Source {
  return { key: `draft:${d.id}`, resolve: () => Promise.resolve(d.path) }
}

const fetches = new Map<string, Promise<string>>()

function fetchCached(path: string, size: number): Promise<string> {
  const key = `${accountId()}|${path}|${size}`
  let p = fetches.get(key)
  if (!p) {
    p = window.talk.fetchFile(path, size)
    p.catch(() => fetches.delete(key))
    fetches.set(key, p)
  }
  return p
}

/**
 * After a voice message ends, the next newer one of the open chat follows (like the Talk
 * apps' queue; Signal only takes directly adjacent ones, but bots answering every voice
 * message would break such chains).
 */
function playNextAfter(done: Source): void {
  if (!done.token || done.messageId == null || done.account !== accountId()) return
  const chat = getState().chats[done.token]
  if (!chat || getState().activeToken !== done.token) return
  const i = chat.messages.findIndex((m) => m.id === done.messageId)
  const next = i >= 0 ? chat.messages.slice(i + 1).find((m) => m.attachment?.voiceMessage && !m.status) : undefined
  if (next) void playSource(messageSource(next), 0)
}

// --- waveforms ------------------------------------------------------------------------

const infos = new Map<string, Promise<AudioInfo>>()

/**
 * Duration and waveform of a message's audio. Received files are fetched first when small
 * enough (`auto`), otherwise only once played.
 */
export function audioInfoFor(src: Source, size: number, auto: boolean): Promise<AudioInfo> | null {
  let p = infos.get(src.key)
  if (p) return p
  if (!auto && size > AUTO_FETCH_BYTES) return null
  p = src.resolve().then((path) => window.talk.audioInfo(path))
  p.catch(() => infos.delete(src.key))
  infos.set(src.key, p)
  return p
}

/** `n` bars (0..1) from the core's 100 levels (mean per bar). */
export function bars(levels: number[], n: number): number[] {
  if (levels.length === 0) return new Array<number>(n).fill(0)
  return Array.from({ length: n }, (_, i) => {
    const a = Math.floor((i * levels.length) / n)
    const b = Math.max(a + 1, Math.floor(((i + 1) * levels.length) / n))
    const part = levels.slice(a, b)
    return part.reduce((x, y) => x + y, 0) / part.length / 255
  })
}

/** 75 000 → "1:15", 3 723 000 → "1:02:03". */
export function clock(ms: number): string {
  const total = Math.max(0, Math.round(ms / 1000))
  const h = Math.floor(total / 3600)
  const m = Math.floor((total % 3600) / 60)
  const s = total % 60
  const ss = String(s).padStart(2, '0')
  return h > 0 ? `${h}:${String(m).padStart(2, '0')}:${ss}` : `${m}:${ss}`
}

export function maxMinutesLeft(durationMs: number): number {
  return Math.max(0, Math.ceil((VOICE_MAX_MS - durationMs) / 60000))
}
