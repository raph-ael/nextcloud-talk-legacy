// Media engine of calls (docs/calls.md, "MediaEngine"): Chromium's WebRTC behind the core's
// commands. The core (main process) decides everything about the protocol; this file only
// runs peer connections and their audio and video; the devices (microphone, camera, speaker)
// are devices.ts, the own screen share is screen.ts.

import { useSyncExternalStore } from 'react'
import type { IceCandidateInit, IceServerConfig, MediaCommand, PeerKind, PeerState } from '@shared/core'
import { stopPlayback, stopToDraft } from '../voice'
import {
  addOutput,
  callStarted,
  callStopped,
  currentTrack,
  deviceSnapshot,
  removeOutput,
  selfLevel,
  setCameraEnabled,
  setMicrophoneEnabled,
  setTrackSink,
  watchDevices,
  type DeviceKind,
} from './devices'
import { currentScreen, screenSnapshot, setScreenSink, stopScreenTrack } from './screen'

/** Level reports to the core (speaking indicator), like the other platforms. */
const LEVEL_MS = 300
/** docs/calls.md „Qualität“, desktop. */
const VIDEO_MAX_BITRATE = 1_500_000
/** docs/calls.md „Bildschirmfreigabe“, „Qualität“. */
const SCREEN_MAX_BITRATE = 2_500_000
const SCREEN_MAX_FPS = 30

const isScreen = (kind: PeerKind) => kind === 'screenPublisher' || kind === 'screenSubscriber'

interface Peer {
  id: string
  kind: PeerKind
  pc: RTCPeerConnection
  /** Commands of this peer run strictly one after another. */
  chain: Promise<void>
  closed: boolean
  /** Remote candidates that came before the remote description. */
  early: RTCIceCandidateInit[]
  /** Our microphone and camera on publisher/mesh connections. */
  senders: Record<DeviceKind, RTCRtpSender | null>
  /** Our screen on a screenPublisher connection (never the camera or microphone). */
  screen: RTCRtpSender | null
  /** Playback of the received audio (subscriber/mesh). */
  audio: HTMLAudioElement | null
  reported: PeerState | null
}

const peers = new Map<string, Peer>()
/** `startAudio` came and no `stopAudio` yet. */
let audioCall: string | null = null
let levelTimer: ReturnType<typeof setInterval> | null = null

// --- Received video (one stream per peer; the call view shows it on the person's tile) ----

const videos = new Map<string, MediaStream>()
const videoListeners = new Set<() => void>()

function setVideo(peerId: string, stream: MediaStream | null): void {
  if (stream) videos.set(peerId, stream)
  else if (!videos.delete(peerId)) return
  videoListeners.forEach((l) => l())
}

/** The video arriving on `peerId` (CallParticipant.peerId), if any. */
export function useRemoteVideo(peerId: string | null): MediaStream | null {
  return useSyncExternalStore(
    (l) => {
      videoListeners.add(l)
      return () => videoListeners.delete(l)
    },
    () => (peerId ? (videos.get(peerId) ?? null) : null),
  )
}

// --- Commands ---------------------------------------------------------------------------

let started = false

/** Connects the engine to the core. Once per renderer. */
export function startMediaEngine(): void {
  if (started) return
  started = true
  setTrackSink(attach)
  setScreenSink(attachScreen)
  watchDevices()
  window.talk.onMediaCommand(handle)
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
      return setMicrophoneEnabled(c.enabled)
    case 'setCameraEnabled':
      return setCameraEnabled(c.enabled)
    case 'startAudio':
      return startAudio(c.callId)
    case 'stopScreen':
      return stopScreenTrack()
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
  const p: Peer = {
    id: peerId,
    kind,
    pc,
    chain: Promise.resolve(),
    closed: false,
    early: [],
    senders: { audio: null, video: null },
    screen: null,
    audio: null,
    reported: null,
  }
  peers.set(peerId, p)
  if (kind === 'screenPublisher') {
    // Only the picture, send only (docs/calls.md „Bildschirmfreigabe“); VP8 first.
    const t = pc.addTransceiver('video', {
      direction: 'sendonly',
      sendEncodings: [{ maxBitrate: SCREEN_MAX_BITRATE, maxFramerate: SCREEN_MAX_FPS }],
    })
    preferVp8(t)
    p.screen = t.sender
    const track = currentScreen()
    if (track) putScreen(p, track)
  } else if (kind === 'publisher' || kind === 'mesh') {
    // Audio and video from the start (docs/calls.md „Video“): the camera is only a track on
    // the video sender, switching it never renegotiates. Tracks follow when the devices are ready.
    const direction = kind === 'publisher' ? 'sendonly' : 'sendrecv'
    p.senders.audio = pc.addTransceiver('audio', { direction }).sender
    p.senders.video = pc.addTransceiver('video', { direction, sendEncodings: [{ maxBitrate: VIDEO_MAX_BITRATE }] }).sender
    for (const k of ['audio', 'video'] as const) {
      const track = currentTrack(k)
      if (track) putTrack(p, k, track)
    }
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
    if (p.closed) return
    if (e.track.kind === 'video') {
      // Own stream per peer: the audio element must not play it, the tile shows it.
      setVideo(p.id, new MediaStream([e.track]))
      return
    }
    if (e.track.kind !== 'audio') return
    if (!p.audio) {
      p.audio = new Audio()
      p.audio.autoplay = true
      addOutput(p.audio)
    }
    p.audio.srcObject = new MediaStream([e.track])
    void p.audio.play().catch(() => undefined)
  }
  startLevels()
}

function preferVp8(t: RTCRtpTransceiver): void {
  const codecs = RTCRtpSender.getCapabilities('video')?.codecs
  if (!codecs) return
  const vp8 = codecs.filter((c) => c.mimeType.toLowerCase() === 'video/vp8')
  if (vp8.length === 0) return
  try {
    t.setCodecPreferences([...vp8, ...codecs.filter((c) => c.mimeType.toLowerCase() !== 'video/vp8')])
  } catch (err) {
    console.warn('screen codecs:', (err as Error).message)
  }
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
  if (type === 'offer' && (p.kind === 'subscriber' || p.kind === 'screenSubscriber')) {
    // Receive only: never send anything to the MCU on a subscriber.
    for (const t of p.pc.getTransceivers()) if (t.direction !== 'stopped') t.direction = 'recvonly'
  } else if (type === 'offer' && p.kind === 'mesh') adoptOffered(p)
  const early = p.early
  p.early = []
  for (const c of early) await addIce(p, c)
}

/**
 * Mesh, the other side offered: an offer only reuses transceivers made by addTrack, ours
 * (addTransceiver) stay unused. Send on the offered ones instead and drop ours.
 */
function adoptOffered(p: Peer): void {
  for (const k of ['audio', 'video'] as const) {
    const offered = p.pc.getTransceivers().find((t) => t.mid !== null && t.receiver.track.kind === k && t.direction !== 'stopped')
    const own = p.senders[k]
    if (!offered || offered.sender === own) continue
    offered.direction = 'sendrecv'
    p.senders[k] = offered.sender
    const track = currentTrack(k)
    if (track) putTrack(p, k, track)
    const unused = p.pc.getTransceivers().find((t) => t.sender === own)
    if (unused && unused.mid === null) unused.stop()
  }
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
    removeOutput(p.audio)
  }
  setVideo(peerId, null)
  if (peers.size === 0) stopLevels()
}

// --- Audio and video tracks ------------------------------------------------------------

function startAudio(callId: string): void {
  audioCall = callId
  // A call takes the audio: voice messages stop (a recording stays as draft).
  stopPlayback()
  void stopToDraft()
  callStarted()
}

function stopAudio(callId: string): void {
  if (audioCall !== callId) return
  audioCall = null
  // Microphone and camera go (docs/calls.md: stopAudio also frees the camera), and the screen.
  callStopped()
  stopScreenTrack()
}

/** A new microphone/camera track (or none) for every sending peer. */
function attach(kind: DeviceKind, track: MediaStreamTrack | null): void {
  for (const p of peers.values()) if (!p.closed) putTrack(p, kind, track)
}

/** The screen track (or none) for every screenPublisher peer. */
function attachScreen(track: MediaStreamTrack | null): void {
  for (const p of peers.values()) if (!p.closed) putScreen(p, track)
}

function putScreen(p: Peer, track: MediaStreamTrack | null): void {
  const sender = p.screen
  if (!sender || sender.track === track) return
  void sender
    .replaceTrack(track)
    .then(() => (track ? tuneScreen(sender) : undefined))
    .catch((err) => console.warn(`peer ${p.id}: screen track:`, (err as Error).message))
}

/** Text stays sharp: under load fewer frames, never fewer pixels. */
async function tuneScreen(sender: RTCRtpSender): Promise<void> {
  const params = sender.getParameters() as RTCRtpSendParameters & { degradationPreference?: string }
  if (params.encodings.length === 0) return
  params.encodings.forEach((e) => {
    e.maxBitrate = SCREEN_MAX_BITRATE
    e.maxFramerate = SCREEN_MAX_FPS
  })
  params.degradationPreference = 'maintain-resolution'
  await sender.setParameters(params).catch((err) => console.warn('screen parameters:', (err as Error).message))
}

function putTrack(p: Peer, kind: DeviceKind, track: MediaStreamTrack | null): void {
  const sender = p.senders[kind]
  if (!sender || sender.track === track) return
  void sender
    .replaceTrack(track)
    .then(() => (kind === 'video' && track ? tuneVideo(sender) : undefined))
    .catch((err) => console.warn(`peer ${p.id}: ${kind} track:`, (err as Error).message))
}

/** At most 1.5 Mbit/s, and under load fewer frames and pixels alike. */
async function tuneVideo(sender: RTCRtpSender): Promise<void> {
  const params = sender.getParameters() as RTCRtpSendParameters & { degradationPreference?: string }
  if (params.encodings.length === 0) return
  params.encodings.forEach((e) => (e.maxBitrate = VIDEO_MAX_BITRATE))
  params.degradationPreference = 'balanced'
  await sender.setParameters(params).catch((err) => console.warn('video parameters:', (err as Error).message))
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
      // A screen's level never counts as speaking (docs/calls.md „Bildschirmfreigabe“).
      if (p.closed || p.reported !== 'connected' || isScreen(p.kind)) return
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
      if (p.closed) return
      if (p.kind === 'publisher') selfLevel(level)
      window.talk.mediaReport({ type: 'audioLevel', peerId: p.id, level })
    }),
  )
}

// --- Diagnostics (tests drive the app over CDP and read this) ---------------------------

async function snapshot(): Promise<unknown> {
  return {
    audioCall,
    ...deviceSnapshot(),
    videos: [...videos.keys()],
    screen: screenSnapshot(),
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
          if (s.type === 'outbound-rtp' && s.kind === 'video') {
            out.videoBytesSent = s.bytesSent
            out.videoSent = `${s.frameWidth ?? 0}x${s.frameHeight ?? 0}@${s.framesPerSecond ?? 0}`
            out.videoLimit = s.qualityLimitationReason
            out.videoCodec = (stats.get(s.codecId as string) as { mimeType?: string } | undefined)?.mimeType
          }
          if (s.type === 'inbound-rtp' && s.kind === 'video') {
            out.videoBytesReceived = s.bytesReceived
            out.framesDecoded = s.framesDecoded
            out.videoReceived = `${s.frameWidth ?? 0}x${s.frameHeight ?? 0}`
          }
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
// Device tests reach the live tracks (ended, unplugged) without real hardware.
;(window as unknown as { __talkTrack: typeof currentTrack }).__talkTrack = currentTrack
