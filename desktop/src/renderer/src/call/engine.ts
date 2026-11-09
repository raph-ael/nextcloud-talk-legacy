// Media engine of calls (docs/calls.md, "MediaEngine"): Chromium's WebRTC behind the core's
// commands. The core (main process) decides everything about the protocol; this file only
// runs peer connections, the microphone and the speaker output.

import type { IceCandidateInit, IceServerConfig, MediaCommand, PeerKind, PeerState } from '@shared/core'
import { stopPlayback, stopToDraft } from '../voice'

/** Level reports to the core (speaking indicator), like the other platforms. */
const LEVEL_MS = 300
const MIC_KEY = 'callMicrophone'
const SPEAKER_KEY = 'callSpeaker'

interface Peer {
  id: string
  kind: PeerKind
  pc: RTCPeerConnection
  /** Commands of this peer run strictly one after another. */
  chain: Promise<void>
  closed: boolean
  /** Remote candidates that came before the remote description. */
  early: RTCIceCandidateInit[]
  /** Our microphone on publisher/mesh connections. */
  sender: RTCRtpSender | null
  /** Playback of the received audio (subscriber/mesh). */
  audio: HTMLAudioElement | null
  reported: PeerState | null
}

const peers = new Map<string, Peer>()
/** `startAudio` came and no `stopAudio` yet. */
let audioCall: string | null = null
let mic: MediaStreamTrack | null = null
/** The microphone request in flight (only one at a time). */
let micRequest: Promise<void> | null = null
let micEnabled = true
let levelTimer: ReturnType<typeof setInterval> | null = null

// --- UI state (microphone problems, devices) -----------------------------------------

export interface MediaState {
  /** Why there is no microphone (shown in the call view); null when it works. */
  micError: string | null
  microphoneId: string
  speakerId: string
}

let ms: MediaState = { micError: null, microphoneId: stored(MIC_KEY), speakerId: stored(SPEAKER_KEY) }
const listeners = new Set<() => void>()

function setMedia(patch: Partial<MediaState>): void {
  ms = { ...ms, ...patch }
  listeners.forEach((l) => l())
}

export function mediaState(): MediaState {
  return ms
}

export function subscribeMedia(l: () => void): () => void {
  listeners.add(l)
  return () => listeners.delete(l)
}

function stored(key: string): string {
  try {
    return localStorage.getItem(key) ?? ''
  } catch {
    return ''
  }
}

function store(key: string, value: string): void {
  try {
    if (value) localStorage.setItem(key, value)
    else localStorage.removeItem(key)
  } catch {
    // Per-device convenience only.
  }
}

// --- Commands ---------------------------------------------------------------------------

let started = false

/** Connects the engine to the core. Once per renderer. */
export function startMediaEngine(): void {
  if (started) return
  started = true
  window.talk.onMediaCommand(handle)
  navigator.mediaDevices.addEventListener('devicechange', () => {
    // A microphone was plugged in after none worked, or the chosen one came back.
    if (audioCall && (!mic || mic.readyState === 'ended')) void acquireMic()
  })
  window.talk.mediaReady()
}

function handle(c: MediaCommand): void {
  switch (c.cmd) {
    case 'createPeer':
      createPeer(c.peerId, c.kind, c.iceServers)
      return
    case 'createOffer':
      return enqueue(c.peerId, (p) => createOffer(p, c.iceRestart))
    case 'createAnswer':
      return enqueue(c.peerId, createAnswer)
    case 'setRemoteDescription':
      return enqueue(c.peerId, (p) => setRemote(p, c.sdpType, c.sdp))
    case 'addRemoteCandidate':
      return enqueue(c.peerId, (p) => addCandidate(p, c.candidate))
    case 'closePeer':
      return closePeer(c.peerId)
    case 'setMicrophoneEnabled':
      micEnabled = c.enabled
      if (mic) mic.enabled = c.enabled
      return
    case 'startAudio':
      return startAudio(c.callId)
    case 'stopAudio':
      return stopAudio(c.callId)
  }
}

function enqueue(peerId: string, op: (p: Peer) => Promise<void>): void {
  const p = peers.get(peerId)
  // Unknown or closed ids are ignored (docs/calls.md rule 2).
  if (!p || p.closed) return
  p.chain = p.chain.then(async () => {
    if (p.closed) return
    try {
      await op(p)
    } catch (err) {
      if (!p.closed) window.talk.mediaReport({ type: 'peerError', peerId: p.id, message: String((err as Error)?.message ?? err) })
    }
  })
}

function rtcConfig(iceServers: IceServerConfig[]): RTCConfiguration {
  return {
    iceServers: iceServers
      .filter((s) => s.urls.length > 0)
      .map((s) => ({ urls: s.urls, username: s.username ?? undefined, credential: s.credential ?? undefined })),
    bundlePolicy: 'max-bundle',
    rtcpMuxPolicy: 'require',
  }
}

function createPeer(peerId: string, kind: PeerKind, iceServers: IceServerConfig[]): void {
  if (peers.has(peerId)) return
  let pc: RTCPeerConnection
  try {
    pc = new RTCPeerConnection(rtcConfig(iceServers))
  } catch (err) {
    window.talk.mediaReport({ type: 'peerError', peerId, message: `createPeer: ${(err as Error).message}` })
    return
  }
  const p: Peer = { id: peerId, kind, pc, chain: Promise.resolve(), closed: false, early: [], sender: null, audio: null, reported: null }
  peers.set(peerId, p)
  if (kind !== 'subscriber') {
    // Audio only; video transceivers come later. The track follows when the mic is ready.
    const t = pc.addTransceiver('audio', { direction: kind === 'publisher' ? 'sendonly' : 'sendrecv' })
    p.sender = t.sender
    if (mic) void t.sender.replaceTrack(mic)
  }
  pc.onicecandidate = (e) => {
    // The end of candidates (null / empty) is not reported.
    if (p.closed || !e.candidate || !e.candidate.candidate) return
    window.talk.mediaReport({
      type: 'localCandidate',
      peerId,
      candidate: { candidate: e.candidate.candidate, sdpMid: e.candidate.sdpMid, sdpMLineIndex: e.candidate.sdpMLineIndex ?? 0 },
    })
  }
  pc.onconnectionstatechange = () => reportState(p)
  pc.oniceconnectionstatechange = () => reportState(p)
  pc.ontrack = (e) => {
    if (p.closed || e.track.kind !== 'audio') return
    const stream = e.streams[0] ?? new MediaStream([e.track])
    if (!p.audio) {
      p.audio = new Audio()
      p.audio.autoplay = true
      void applySink(p.audio, ms.speakerId)
    }
    p.audio.srcObject = stream
    void p.audio.play().catch(() => undefined)
  }
  startLevels()
}

/** connectionState, with `failed` from either it or the ICE state (rule 7). */
function stateOf(pc: RTCPeerConnection): PeerState {
  const c = pc.connectionState
  const i = pc.iceConnectionState
  if (c === 'failed' || i === 'failed') return 'failed'
  if (c === 'closed' || i === 'closed') return 'closed'
  if (c === 'connected') return 'connected'
  if (c === 'disconnected' || i === 'disconnected') return 'disconnected'
  if (c === 'connecting' || i === 'checking') return 'connecting'
  return 'new'
}

function reportState(p: Peer): void {
  if (p.closed) return
  const state = stateOf(p.pc)
  if (state === p.reported) return
  p.reported = state
  window.talk.mediaReport({ type: 'peerState', peerId: p.id, state })
}

async function createOffer(p: Peer, iceRestart: boolean): Promise<void> {
  const offer = await p.pc.createOffer({ iceRestart })
  await p.pc.setLocalDescription(offer)
  reportLocal(p, 'offer')
}

async function createAnswer(p: Peer): Promise<void> {
  const answer = await p.pc.createAnswer()
  await p.pc.setLocalDescription(answer)
  reportLocal(p, 'answer')
}

function reportLocal(p: Peer, sdpType: 'offer' | 'answer'): void {
  const sdp = p.pc.localDescription?.sdp
  if (p.closed || !sdp) return
  window.talk.mediaReport({ type: 'localDescription', peerId: p.id, sdpType, sdp })
}

async function setRemote(p: Peer, type: 'offer' | 'answer', sdp: string): Promise<void> {
  await p.pc.setRemoteDescription({ type, sdp })
  if (type === 'offer' && p.kind === 'subscriber') {
    // Receive only: never send anything to the MCU on a subscriber.
    for (const t of p.pc.getTransceivers()) if (t.direction !== 'stopped') t.direction = 'recvonly'
  }
  const early = p.early
  p.early = []
  for (const c of early) await addIce(p, c)
}

async function addCandidate(p: Peer, c: IceCandidateInit): Promise<void> {
  const init: RTCIceCandidateInit = { candidate: c.candidate, sdpMid: c.sdpMid ?? undefined, sdpMLineIndex: c.sdpMLineIndex }
  if (!p.pc.remoteDescription) p.early.push(init)
  else await addIce(p, init)
}

async function addIce(p: Peer, c: RTCIceCandidateInit): Promise<void> {
  try {
    await p.pc.addIceCandidate(c)
  } catch (err) {
    // One unusable candidate is no reason to tear the connection down (the core would).
    console.warn(`peer ${p.id}: candidate rejected:`, (err as Error).message)
  }
}

function closePeer(peerId: string): void {
  const p = peers.get(peerId)
  if (!p) return
  p.closed = true
  peers.delete(peerId)
  try {
    p.pc.close()
  } catch {
    // already closed
  }
  if (p.audio) {
    p.audio.pause()
    p.audio.srcObject = null
  }
  if (peers.size === 0) stopLevels()
}

// --- Audio ------------------------------------------------------------------------------

function startAudio(callId: string): void {
  audioCall = callId
  // A call takes the audio: voice messages stop (a recording stays as draft).
  stopPlayback()
  void stopToDraft()
  void acquireMic()
}

function stopAudio(callId: string): void {
  if (audioCall !== callId) return
  audioCall = null
  releaseMic()
  setMedia({ micError: null })
}

function micConstraints(deviceId: string): MediaStreamConstraints {
  return {
    audio: {
      echoCancellation: true,
      noiseSuppression: true,
      autoGainControl: true,
      ...(deviceId ? { deviceId: { ideal: deviceId } } : {}),
    },
    video: false,
  }
}

/** Opens the microphone (the chosen one, else the default) and puts it on every sender. */
function acquireMic(): Promise<void> {
  if (micRequest) return micRequest
  const call = audioCall
  micRequest = (async () => {
    try {
      const stream = await navigator.mediaDevices.getUserMedia(micConstraints(ms.microphoneId))
      const track = stream.getAudioTracks()[0]
      if (!track) throw new Error('Kein Mikrofon gefunden')
      if (audioCall !== call || !audioCall) {
        track.stop()
        return
      }
      useTrack(track)
      setMedia({ micError: null })
    } catch (err) {
      const name = (err as DOMException)?.name
      const text =
        name === 'NotAllowedError'
          ? 'Kein Zugriff auf das Mikrofon'
          : name === 'NotFoundError'
            ? 'Kein Mikrofon gefunden'
            : `Mikrofon nicht verfügbar (${(err as Error).message})`
      console.warn('microphone:', err)
      if (audioCall === call) setMedia({ micError: text })
    } finally {
      micRequest = null
    }
  })()
  return micRequest
}

function useTrack(track: MediaStreamTrack): void {
  const old = mic
  mic = track
  track.enabled = micEnabled
  // Unplugged mid-call: fall back to another microphone.
  track.onended = () => {
    if (mic === track && audioCall) {
      mic = null
      void acquireMic()
    }
  }
  for (const p of peers.values()) if (p.sender && !p.closed) void p.sender.replaceTrack(track).catch(() => undefined)
  old?.stop()
}

function releaseMic(): void {
  const old = mic
  mic = null
  for (const p of peers.values()) if (p.sender && !p.closed) void p.sender.replaceTrack(null).catch(() => undefined)
  old?.stop()
}

// --- Devices ----------------------------------------------------------------------------

export interface AudioDevices {
  microphones: MediaDeviceInfo[]
  speakers: MediaDeviceInfo[]
}

export async function listDevices(): Promise<AudioDevices> {
  const all = await navigator.mediaDevices.enumerateDevices()
  const real = (d: MediaDeviceInfo) => d.deviceId !== 'default' && d.deviceId !== 'communications' && d.deviceId !== ''
  return {
    microphones: all.filter((d) => d.kind === 'audioinput' && real(d)),
    speakers: all.filter((d) => d.kind === 'audiooutput' && real(d)),
  }
}

/** '' is the system default. Switches a running call at once (no renegotiation). */
export function chooseMicrophone(deviceId: string): void {
  store(MIC_KEY, deviceId)
  setMedia({ microphoneId: deviceId })
  if (!audioCall) return
  void (async () => {
    try {
      const stream = await navigator.mediaDevices.getUserMedia(micConstraints(deviceId))
      const track = stream.getAudioTracks()[0]
      if (track && audioCall) useTrack(track)
      else track?.stop()
    } catch (err) {
      console.warn('switching microphone failed:', err)
    }
  })()
}

export function chooseSpeaker(deviceId: string): void {
  store(SPEAKER_KEY, deviceId)
  setMedia({ speakerId: deviceId })
  for (const p of peers.values()) if (p.audio) void applySink(p.audio, deviceId)
}

/** Output device for call audio and call tones ('' = system default). */
export function speakerId(): string {
  return ms.speakerId
}

export async function applySink(el: HTMLMediaElement | AudioContext, deviceId: string): Promise<void> {
  const target = el as unknown as { setSinkId?: (id: string) => Promise<void> }
  if (!target.setSinkId) return
  try {
    await target.setSinkId(deviceId)
  } catch (err) {
    // Device gone: the system default plays.
    console.warn('output device:', (err as Error).message)
    if (deviceId) await target.setSinkId('').catch(() => undefined)
  }
}

// --- Levels -----------------------------------------------------------------------------

function startLevels(): void {
  if (levelTimer) return
  levelTimer = setInterval(() => void reportLevels(), LEVEL_MS)
}

function stopLevels(): void {
  if (levelTimer) clearInterval(levelTimer)
  levelTimer = null
}

async function reportLevels(): Promise<void> {
  await Promise.all(
    [...peers.values()].map(async (p) => {
      if (p.closed || p.reported !== 'connected') return
      // Publisher: our microphone; subscriber/mesh: what arrives.
      const wanted = p.kind === 'publisher' ? 'media-source' : 'inbound-rtp'
      let level = 0
      try {
        const stats = await p.pc.getStats()
        stats.forEach((s: { type: string; kind?: string; audioLevel?: number }) => {
          if (s.type === wanted && s.kind === 'audio' && typeof s.audioLevel === 'number') level = Math.max(level, s.audioLevel)
        })
      } catch {
        return
      }
      if (!p.closed) window.talk.mediaReport({ type: 'audioLevel', peerId: p.id, level })
    }),
  )
}

// --- Diagnostics (tests drive the app over CDP and read this) ---------------------------

async function snapshot(): Promise<unknown> {
  return {
    audioCall,
    mic: mic ? { label: mic.label, enabled: mic.enabled, state: mic.readyState } : null,
    micError: ms.micError,
    peers: await Promise.all(
      [...peers.values()].map(async (p) => {
        const out: Record<string, unknown> = {
          id: p.id,
          kind: p.kind,
          connection: p.pc.connectionState,
          ice: p.pc.iceConnectionState,
          reported: p.reported,
          playing: p.audio ? !p.audio.paused : null,
        }
        const stats = await p.pc.getStats().catch(() => null)
        stats?.forEach((s: Record<string, unknown>) => {
          if (s.type === 'outbound-rtp' && s.kind === 'audio') out.bytesSent = s.bytesSent
          if (s.type === 'inbound-rtp' && s.kind === 'audio') {
            out.bytesReceived = s.bytesReceived
            out.packetsLost = s.packetsLost
            out.jitter = s.jitter
            out.inLevel = s.audioLevel
          }
          if (s.type === 'media-source' && s.kind === 'audio') out.micLevel = s.audioLevel
          if (s.type === 'candidate-pair' && s.nominated && s.state === 'succeeded') {
            const local = stats.get(s.localCandidateId as string) as Record<string, unknown> | undefined
            const remote = stats.get(s.remoteCandidateId as string) as Record<string, unknown> | undefined
            out.pair = `${local?.candidateType}/${local?.protocol} → ${remote?.candidateType}/${remote?.protocol}`
          }
        })
        return out
      }),
    ),
  }
}

;(window as unknown as { __talkMedia: () => Promise<unknown> }).__talkMedia = snapshot
