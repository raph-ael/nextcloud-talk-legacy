// Call UI in the main window, after Signal Desktop: incoming-call bar on top, full call view
// (1:1 big avatar or their video, group tiles), own camera preview, control bar, "back to
// call" bar while minimized, device menu like Google Meet.

import { useEffect, useRef, useState, useSyncExternalStore, type ReactNode, type RefObject } from 'react'
import type { CallParticipant, CallView, RoomView } from '@shared/core'
import { useStore } from '../store'
import { Avatar } from '../components/Avatar'
import {
  accept,
  consentToRecording,
  decline,
  duration,
  hangUp,
  isRecording,
  minimize,
  startCall,
  startRecording,
  statusText,
  stopRecording,
  toggleCamera,
  toggleMute,
  useCall,
} from './store'
import { useRemoteVideo } from './engine'
import {
  chooseCamera,
  chooseMicrophone,
  cameraText,
  chooseSpeaker,
  currentTrack,
  listDevices,
  mediaState,
  openCameraPreview,
  retryCamera,
  retryMicrophone,
  subscribeMedia,
  type DeviceProblem,
  type Devices,
} from './devices'
import { playTestTone } from './tones'

const isMac = navigator.platform.toLowerCase().includes('mac')
const MOD = isMac ? '⌘' : 'Strg'

/** Everything call-related on top of the app. */
export function CallLayer() {
  const call = useCall((s) => s.call)
  const minimized = useCall((s) => s.minimized)
  const toast = useCall((s) => s.toast)
  return (
    <>
      {call?.state === 'incoming' && <IncomingCallBar call={call} />}
      {call && call.state !== 'incoming' && !minimized && <CallScreen call={call} />}
      {/* Always mounted: a live region only announces changes, not its own appearance. */}
      <div className="call-toast-region" role="status" aria-live="polite">
        {toast && <div className="call-toast">{toast}</div>}
      </div>
    </>
  )
}

/** Above the app while the call view is minimized: "Zurück zum Anruf · 03:12". */
export function ReturnToCallBar() {
  const call = useCall((s) => s.call)
  const minimized = useCall((s) => s.minimized)
  if (!call || !minimized || call.state === 'incoming') return null
  return (
    <div className="call-return">
      <button className="call-return-main" onClick={() => minimize(false)}>
        <PhoneIcon size={16} />
        <span className="ellipsis">
          Zurück zum Anruf · <CallStatus call={call} />
        </span>
        {call.recording !== 'off' && call.recording !== 'failed' && (
          <span className={`call-return-rec${isRecording(call) ? '' : ' starting'}`} title={recordingText(call) ?? undefined}>
            ● REC
          </span>
        )}
      </button>
      <button className="call-return-hangup" onClick={hangUp} title={`Auflegen (${MOD}+Umschalt+D)`} aria-label="Auflegen">
        <HangUpIcon size={16} />
      </button>
    </div>
  )
}

/** Call button in the chat header; "Beitreten" while the conversation has a running call. */
export function CallButton({ room }: { room: RoomView }) {
  const call = useCall((s) => s.call)
  // 1:1, groups and public conversations; not changelog, former 1:1 or notes to self.
  if (![1, 2, 3].includes(room.roomType)) return null
  const live = call && call.state !== 'ended' && call.state !== 'incoming' ? call : null
  const here = live?.token === room.token
  const busy = !!live && !here
  if (room.hasCall && !here && !busy) {
    return (
      <button className="call-join" onClick={() => startCall(room.token)} title="Dem laufenden Anruf beitreten">
        <PhoneIcon size={16} />
        Beitreten
      </button>
    )
  }
  return (
    <button
      className="header-icon-btn"
      onClick={() => (here ? minimize(false) : startCall(room.token))}
      disabled={busy}
      title={busy ? 'Es läuft bereits ein Anruf' : here ? 'Zurück zum Anruf' : 'Sprachanruf'}
      aria-label={here ? 'Zurück zum Anruf' : 'Sprachanruf starten'}
    >
      <PhoneIcon size={20} />
    </button>
  )
}

// --- Incoming ---------------------------------------------------------------------------

function IncomingCallBar({ call }: { call: CallView }) {
  const room = useStore((s) => s.rooms.find((r) => r.token === call.token))
  const sub = call.isGroup ? (call.callerName ? `${call.callerName} · Gruppenanruf` : 'Gruppenanruf') : 'Eingehender Anruf'
  return (
    <div className="call-incoming" role="alertdialog" aria-label={`Anruf von ${call.roomName}`}>
      <Avatar name={call.roomName} token={call.token} version={room?.avatarVersion} size={48} />
      <div className="call-incoming-text">
        <strong className="ellipsis">{call.roomName}</strong>
        <span className="muted ellipsis">{sub}</span>
      </div>
      <button
        className="call-round call-decline"
        onClick={decline}
        title={`Ablehnen (${MOD}+Umschalt+D)`}
        aria-label="Ablehnen"
      >
        <HangUpIcon />
      </button>
      <button className="call-round call-accept" onClick={accept} title={`Annehmen (${MOD}+Umschalt+A)`} aria-label="Annehmen">
        <PhoneIcon />
      </button>
    </div>
  )
}

// --- Call view --------------------------------------------------------------------------

function CallScreen({ call }: { call: CallView }) {
  const room = useStore((s) => s.rooms.find((r) => r.token === call.token))
  const media = useSyncExternalStore(subscribeMedia, mediaState)
  const tiles = call.isGroup && call.participants.length > 0
  const ended = call.state === 'ended'
  const [confirmStop, setConfirmStop] = useState(false)
  const consent = call.recordingConsentPending && !ended
  // 1:1: the other side's video fills the stage while it shows pictures.
  const other = tiles ? null : (call.participants.find((p) => p.video) ?? call.participants[0] ?? null)
  const otherStream = useRemoteVideo(other?.video ? other.peerId : null)
  const [otherRef, otherLive] = useVideoLive(otherStream)
  const soloVideo = !tiles && otherLive && !ended
  const full = useFullscreen()
  const self = call.cameraOn && !ended ? media.camera : null
  return (
    <div
      className={`call-screen${ended ? ' ended' : ''}${soloVideo ? ' solo-video' : ''}`}
      role="dialog"
      aria-label={`Anruf mit ${call.roomName}`}
    >
      <div
        className="call-backdrop"
        style={{ backgroundImage: `url("${window.talk.avatarUrl(call.token, room?.avatarVersion ?? '')}")` }}
      />
      <header className="call-top">
        <button className="call-icon-btn" onClick={() => minimize(true)} title="Zu den Chats" aria-label="Anruf verkleinern">
          <MinimizeIcon />
        </button>
        {(tiles || soloVideo) && (
          <div className="call-top-title">
            <strong className="ellipsis">{call.roomName}</strong>
            <CallStatusLine call={call} />
          </div>
        )}
        <div className="call-top-end">
          {!ended && <RecordingIndicator call={call} />}
          <button
            className="call-icon-btn"
            onClick={toggleFullscreen}
            title={full ? 'Vollbild beenden (Esc)' : 'Vollbild'}
            aria-label={full ? 'Vollbild beenden' : 'Vollbild'}
          >
            {full ? <ExitFullscreenIcon /> : <FullscreenIcon />}
          </button>
        </div>
      </header>

      <main className="call-stage" onDoubleClick={(e) => (e.target as HTMLElement).closest('button') || toggleFullscreen()}>
        {otherStream && !tiles && (
          <video ref={otherRef} className={`call-stage-video${soloVideo ? ' live' : ''}`} autoPlay muted playsInline />
        )}
        {tiles ? (
          <div className={`call-tiles n${Math.min(call.participants.length, 9)}`}>
            {call.participants.map((p) => (
              <Tile key={p.sessionId} p={p} />
            ))}
          </div>
        ) : (
          !soloVideo && (
            <div className="call-solo">
              <Avatar name={call.roomName} token={call.token} version={room?.avatarVersion} size={112} />
              <h1 className="ellipsis">{call.roomName}</h1>
              <CallStatusLine call={call} />
            </div>
          )
        )}
        {self && <SelfView stream={self} speaking={media.selfSpeaking && !call.muted} />}
        {!ended && (media.micError || media.camError) && (
          <div className="call-notices">
            {media.micError && <Notice problem={media.micError} onRetry={retryMicrophone} />}
            {media.camError && <Notice problem={media.camError} onRetry={retryCamera} />}
          </div>
        )}
      </main>

      <footer className="call-controls">
        <DevicesButton disabled={ended} />
        <button
          // Off is the normal state (every call starts without camera): crossed out, not highlighted.
          className="call-ctl"
          onClick={toggleCamera}
          disabled={ended}
          title={`${call.cameraOn ? 'Kamera ausschalten' : 'Kamera einschalten'} (${MOD}+Umschalt+V)`}
          aria-label={call.cameraOn ? 'Kamera ausschalten' : 'Kamera einschalten'}
          aria-pressed={call.cameraOn}
        >
          {call.cameraOn ? <CameraIcon /> : <CameraOffIcon />}
        </button>
        {call.canRecord && !ended && <MoreButton call={call} onStop={() => setConfirmStop(true)} />}
        <button
          className={`call-ctl${call.muted ? ' active' : ''}`}
          onClick={toggleMute}
          disabled={ended}
          title={`${call.muted ? 'Stummschaltung aufheben' : 'Stummschalten'} (${MOD}+Umschalt+M)`}
          aria-label={call.muted ? 'Stummschaltung aufheben' : 'Stummschalten'}
          aria-pressed={call.muted}
        >
          {call.muted ? <MicOffIcon /> : <MicIcon />}
        </button>
        <button
          className="call-ctl call-hangup"
          onClick={hangUp}
          disabled={ended}
          title={`Auflegen (${MOD}+Umschalt+D)`}
          aria-label="Auflegen"
        >
          <HangUpIcon />
        </button>
      </footer>

      {consent && (
        <CallDialog
          title="Dieser Anruf kann aufgezeichnet werden"
          text="Wenn du beitrittst, stimmst du der Aufzeichnung zu."
          cancel="Abbrechen"
          confirm="Zustimmen und beitreten"
          onCancel={hangUp}
          onConfirm={consentToRecording}
        />
      )}
      {confirmStop && !consent && (
        <CallDialog
          title="Aufnahme beenden?"
          cancel="Abbrechen"
          confirm="Beenden"
          onCancel={() => setConfirmStop(false)}
          onConfirm={() => {
            setConfirmStop(false)
            stopRecording()
          }}
        />
      )}
    </div>
  )
}

// --- Recording (docs/calls.md, „Aufnahme“) -------------------------------------------

/** Shown text of the recording state; `null`: nothing to show. */
function recordingText(call: CallView): string | null {
  switch (call.recording) {
    case 'startingAudio':
    case 'startingVideo':
      return 'Aufnahme startet …'
    case 'audio':
    case 'video':
      return 'Aufnahme läuft'
    case 'failed':
      // Only those who can start it again need to know.
      return call.canRecord ? 'Aufnahme fehlgeschlagen' : null
    default:
      return null
  }
}

function RecordingIndicator({ call }: { call: CallView }) {
  const text = recordingText(call)
  if (!text) return null
  const kind = isRecording(call) ? 'on' : call.recording === 'failed' ? 'failed' : 'starting'
  return (
    <div className={`call-rec ${kind}`}>
      <span className="call-rec-dot" aria-hidden />
      {text}
    </div>
  )
}

/** „Mehr“ (⋯): start (audio/video) or stop the recording. */
function MoreButton({ call, onStop }: { call: CallView; onStop: () => void }) {
  const [open, setOpen] = useState(false)
  const ref = usePopover(open, () => setOpen(false))
  const running = call.recording !== 'off' && call.recording !== 'failed'
  const pick = (f: () => void) => () => {
    setOpen(false)
    f()
  }
  return (
    <div className="call-devices" ref={ref}>
      <button
        className={`call-ctl${open ? ' active' : ''}`}
        onClick={() => setOpen((o) => !o)}
        title="Mehr"
        aria-label="Mehr"
        aria-haspopup="menu"
        aria-expanded={open}
      >
        <MoreIcon />
      </button>
      {open && (
        <div className="call-menu narrow" role="menu">
          {running ? (
            <button className="call-menu-item" role="menuitem" onClick={pick(onStop)}>
              <span className="call-menu-check rec">●</span>
              Aufnahme beenden
            </button>
          ) : (
            <>
              <button className="call-menu-item" role="menuitem" onClick={pick(() => startRecording(false))}>
                <span className="call-menu-check rec">●</span>
                Audio aufnehmen
              </button>
              <button className="call-menu-item" role="menuitem" onClick={pick(() => startRecording(true))}>
                <span className="call-menu-check rec">●</span>
                Video aufnehmen
              </button>
            </>
          )}
        </div>
      )}
    </div>
  )
}

/** Modal question on top of the call view; Escape cancels, Enter confirms. */
function CallDialog(p: {
  title: string
  text?: string
  cancel: string
  confirm: string
  onCancel: () => void
  onConfirm: () => void
}) {
  const confirmRef = useRef<HTMLButtonElement>(null)
  const cancelRef = useRef(p.onCancel)
  cancelRef.current = p.onCancel
  useEffect(() => {
    // Focus once on open (the call view re-renders often).
    confirmRef.current?.focus()
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return
      e.stopPropagation()
      cancelRef.current()
    }
    window.addEventListener('keydown', onKey, true)
    return () => window.removeEventListener('keydown', onKey, true)
  }, [])
  return (
    <div className="call-dialog-backdrop">
      <div className="call-dialog" role="alertdialog" aria-modal="true" aria-labelledby="call-dialog-title">
        <h2 id="call-dialog-title">{p.title}</h2>
        {p.text && <p>{p.text}</p>}
        <div className="call-dialog-buttons">
          <button className="call-dialog-btn" onClick={p.onCancel}>
            {p.cancel}
          </button>
          <button className="call-dialog-btn primary" ref={confirmRef} onClick={p.onConfirm}>
            {p.confirm}
          </button>
        </div>
      </div>
    </div>
  )
}

function CallStatusLine({ call }: { call: CallView }) {
  return (
    <p className={`call-status${call.state === 'reconnecting' ? ' blink' : ''}`} aria-live="polite">
      <CallStatus call={call} />
    </p>
  )
}

/** Status text, or the running timer while connected. */
function CallStatus({ call }: { call: CallView }) {
  const text = statusText(call)
  const now = useNow(text === null)
  if (text !== null) return <>{text}</>
  return <>{duration(now - (call.startedAt ?? now))}</>
}

function useNow(running: boolean): number {
  const [now, setNow] = useState(Date.now)
  useEffect(() => {
    if (!running) return
    setNow(Date.now())
    const t = setInterval(() => setNow(Date.now()), 1000)
    return () => clearInterval(t)
  }, [running])
  return now
}

function Tile({ p }: { p: CallParticipant }) {
  const stream = useRemoteVideo(p.video ? p.peerId : null)
  const [ref, live] = useVideoLive(stream)
  return (
    <div
      className={`call-tile${p.speaking && !p.muted ? ' speaking' : ''}${p.connected ? '' : ' waiting'}${live ? ' has-video' : ''}`}
    >
      {stream && <video ref={ref} className={`call-video${live ? ' live' : ''}`} autoPlay muted playsInline />}
      {!live && <Avatar name={p.name} userId={p.userId || undefined} size={72} />}
      <div className="call-tile-name">
        {p.muted && (
          <span className="call-tile-muted" title="Stummgeschaltet">
            <MicOffIcon size={14} />
          </span>
        )}
        <span className="ellipsis">{p.name}</span>
      </div>
      {!p.connected && <span className="call-tile-wait">Verbinde …</span>}
    </div>
  )
}

// --- Video ------------------------------------------------------------------------------

/** No picture for this long: the avatar again (docs/calls.md „Video“). */
const FRAME_TIMEOUT_MS = 2000

/**
 * Plays `stream` in the returned video element; `live` while pictures arrive (the track is
 * not muted and the last frame is under 2 s old). The element stays mounted (and decoding)
 * while hidden, so the switch back is immediate.
 */
function useVideoLive(stream: MediaStream | null): [RefObject<HTMLVideoElement | null>, boolean] {
  const ref = useRef<HTMLVideoElement>(null)
  const [live, setLive] = useState(false)
  useEffect(() => {
    const v = ref.current
    const track = stream?.getVideoTracks()[0]
    if (!v || !stream || !track) {
      setLive(false)
      return
    }
    v.srcObject = stream
    void v.play().catch(() => undefined)
    let last = 0
    let handle = 0
    let stopped = false
    const onFrame = () => {
      last = performance.now()
      if (!stopped) handle = v.requestVideoFrameCallback(onFrame)
    }
    handle = v.requestVideoFrameCallback(onFrame)
    const check = () => setLive(!track.muted && track.readyState === 'live' && performance.now() - last < FRAME_TIMEOUT_MS)
    const timer = setInterval(check, 250)
    track.addEventListener('mute', check)
    track.addEventListener('unmute', check)
    return () => {
      stopped = true
      v.cancelVideoFrameCallback(handle)
      clearInterval(timer)
      track.removeEventListener('mute', check)
      track.removeEventListener('unmute', check)
      v.srcObject = null
    }
  }, [stream])
  return [ref, live]
}

/** Own camera, bottom right, mirrored like a mirror; speaking frame from our level. */
function SelfView({ stream, speaking }: { stream: MediaStream; speaking: boolean }) {
  const [ref] = useVideoLive(stream)
  return (
    <div className={`call-self${speaking ? ' speaking' : ''}`} aria-label="Eigene Kamera">
      <video ref={ref} className="call-video live mirrored" autoPlay muted playsInline />
    </div>
  )
}

function Notice({ problem, onRetry }: { problem: DeviceProblem; onRetry: () => void }) {
  return (
    <div className="call-notice" role="alert">
      {problem.text}
      {problem.retry && (
        <button className="call-notice-btn" onClick={onRetry}>
          Erneut versuchen
        </button>
      )}
    </div>
  )
}

// --- Fullscreen (button, double click on the stage, Esc) --------------------------------

/** The whole window (toasts and menus stay visible), like the window menu's „Vollbild“. */
function toggleFullscreen(): void {
  if (document.fullscreenElement) void document.exitFullscreen().catch(() => undefined)
  else void document.documentElement.requestFullscreen().catch(() => undefined)
}

function useFullscreen(): boolean {
  const [full, setFull] = useState(() => !!document.fullscreenElement)
  useEffect(() => {
    const onChange = () => setFull(!!document.fullscreenElement)
    const onKey = (e: KeyboardEvent) => {
      // A menu or dialog closes first.
      if (e.key !== 'Escape' || !document.fullscreenElement || document.querySelector('.call-menu, .call-dialog')) return
      void document.exitFullscreen().catch(() => undefined)
    }
    document.addEventListener('fullscreenchange', onChange)
    window.addEventListener('keydown', onKey)
    return () => {
      document.removeEventListener('fullscreenchange', onChange)
      window.removeEventListener('keydown', onKey)
      // The call view goes (ended, minimized): the window leaves fullscreen with it.
      if (document.fullscreenElement) void document.exitFullscreen().catch(() => undefined)
    }
  }, [])
  return full
}

// --- Devices ----------------------------------------------------------------------------

function DevicesButton({ disabled }: { disabled: boolean }) {
  const [open, setOpen] = useState(false)
  const ref = usePopover(open, () => setOpen(false))
  return (
    <div className="call-devices" ref={ref}>
      <button
        className={`call-ctl${open ? ' active' : ''}`}
        onClick={() => setOpen((o) => !o)}
        disabled={disabled}
        title="Mikrofon, Lautsprecher und Kamera"
        aria-label="Mikrofon, Lautsprecher und Kamera"
        aria-expanded={open}
      >
        <SpeakerIcon />
      </button>
      {open && <DevicesMenu />}
    </div>
  )
}

/** Closes an open popover on a click outside or Escape. */
function usePopover(open: boolean, close: () => void) {
  const ref = useRef<HTMLDivElement>(null)
  const closeRef = useRef(close)
  closeRef.current = close
  useEffect(() => {
    if (!open) return
    const onDown = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) closeRef.current()
    }
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && closeRef.current()
    window.addEventListener('mousedown', onDown)
    window.addEventListener('keydown', onKey)
    return () => {
      window.removeEventListener('mousedown', onDown)
      window.removeEventListener('keydown', onKey)
    }
  }, [open])
  return ref
}

function DevicesMenu() {
  const media = useSyncExternalStore(subscribeMedia, mediaState)
  const [devices, setDevices] = useState<Devices | null>(null)
  useEffect(() => {
    let live = true
    const load = () =>
      void listDevices()
        .then((d) => live && setDevices(d))
        .catch(() => undefined)
    load()
    navigator.mediaDevices.addEventListener('devicechange', load)
    return () => {
      live = false
      navigator.mediaDevices.removeEventListener('devicechange', load)
    }
  }, [])
  const section = (
    title: string,
    list: MediaDeviceInfo[] | undefined,
    current: string,
    choose: (id: string) => void,
    extra: ReactNode,
  ) => (
    <div className="call-menu-section" role="group" aria-label={title}>
      <div className="call-menu-title">{title}</div>
      {[{ deviceId: '', label: 'Systemstandard' }, ...(list ?? [])].map((d, i) => (
        <button
          key={d.deviceId || 'default'}
          className="call-menu-item"
          role="menuitemradio"
          aria-checked={current === d.deviceId}
          onClick={() => choose(d.deviceId)}
        >
          <span className="call-menu-check">{current === d.deviceId ? '✓' : ''}</span>
          <span className="ellipsis">{d.label || `Gerät ${i}`}</span>
        </button>
      ))}
      {extra}
    </div>
  )
  return (
    <div className="call-menu" role="menu">
      {section('Mikrofon', devices?.microphones, media.microphoneId, chooseMicrophone, <MicLevel />)}
      {section(
        'Lautsprecher',
        devices?.speakers,
        media.speakerId,
        chooseSpeaker,
        <button className="call-menu-action" onClick={playTestTone}>
          <SpeakerIcon size={16} />
          Testton abspielen
        </button>,
      )}
      {section('Kamera', devices?.cameras, media.cameraId, chooseCamera, <CameraPreview />)}
    </div>
  )
}

/** Live level of the microphone the call sends (silent while muted). */
function MicLevel() {
  useSyncExternalStore(subscribeMedia, mediaState)
  const track = currentTrack('audio')
  const bar = useRef<HTMLDivElement>(null)
  useEffect(() => {
    if (!track) return
    const ctx = new AudioContext()
    const analyser = ctx.createAnalyser()
    analyser.fftSize = 512
    ctx.createMediaStreamSource(new MediaStream([track])).connect(analyser)
    const data = new Float32Array(analyser.fftSize)
    let raf = 0
    const tick = () => {
      analyser.getFloatTimeDomainData(data)
      let sum = 0
      for (const x of data) sum += x * x
      // RMS on a speech-friendly scale (about -50…0 dBFS).
      const db = 20 * Math.log10(Math.sqrt(sum / data.length) || 1e-6)
      const level = Math.min(1, Math.max(0, (db + 50) / 50))
      if (bar.current) bar.current.style.transform = `scaleX(${level})`
      raf = requestAnimationFrame(tick)
    }
    tick()
    return () => {
      cancelAnimationFrame(raf)
      void ctx.close()
    }
  }, [track])
  if (!track) return null
  return (
    <div className="call-meter" aria-hidden>
      <div className="call-meter-bar" ref={bar} />
    </div>
  )
}

/**
 * The running camera; while it is off a preview only on request (the camera light should
 * not go on just because the menu opened).
 */
function CameraPreview() {
  const media = useSyncExternalStore(subscribeMedia, mediaState)
  const [wanted, setWanted] = useState(false)
  const [preview, setPreview] = useState<MediaStream | null>(null)
  const [error, setError] = useState<string | null>(null)
  const running = media.camera
  useEffect(() => {
    if (running || !wanted) return
    let live = true
    let stream: MediaStream | null = null
    setError(null)
    openCameraPreview(media.cameraId)
      .then((s) => {
        if (live) {
          stream = s
          setPreview(s)
        } else s.getTracks().forEach((t) => t.stop())
      })
      .catch((err) => live && setError(cameraText(err)))
    return () => {
      live = false
      stream?.getTracks().forEach((t) => t.stop())
      setPreview(null)
    }
  }, [running, wanted, media.cameraId])
  const stream = running ?? preview
  const [ref] = useVideoLive(stream)
  if (!stream) {
    return (
      <>
        {error && <div className="call-menu-error">{error}</div>}
        <button className="call-menu-action" onClick={() => setWanted(true)} disabled={wanted && !error}>
          <CameraIcon size={16} />
          Vorschau anzeigen
        </button>
      </>
    )
  }
  return <video ref={ref} className="call-menu-preview mirrored" autoPlay muted playsInline />
}

// --- Icons (own drawings) ---------------------------------------------------------------

function Svg({ size = 22, children }: { size?: number; children: ReactNode }) {
  return (
    <svg
      viewBox="0 0 24 24"
      width={size}
      height={size}
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

export function PhoneIcon({ size }: { size?: number }) {
  return (
    <Svg size={size}>
      <path d="M6.6 3.5h2.6l1.4 4-2 1.5a11 11 0 0 0 6.4 6.4l1.5-2 4 1.4v2.6a2 2 0 0 1-2.2 2A16.5 16.5 0 0 1 4.6 5.7a2 2 0 0 1 2-2.2Z" />
    </Svg>
  )
}

function HangUpIcon({ size }: { size?: number }) {
  return (
    <Svg size={size}>
      <path d="M3.2 13.6c4.9-4.4 12.7-4.4 17.6 0l-.2 2.7-3.6.4-.8-2.4a9 9 0 0 0-8.4 0l-.8 2.4-3.6-.4Z" />
    </Svg>
  )
}

function MicIcon({ size }: { size?: number }) {
  return (
    <Svg size={size}>
      <rect x="9" y="3" width="6" height="11" rx="3" />
      <path d="M5.5 11a6.5 6.5 0 0 0 13 0M12 17.5V21" />
    </Svg>
  )
}

function MicOffIcon({ size }: { size?: number }) {
  return (
    <Svg size={size}>
      <path d="M15 10V6a3 3 0 0 0-5.8-1.1M9 9v2a3 3 0 0 0 4.6 2.5M5.5 11a6.5 6.5 0 0 0 10.4 5.2M18.5 11a6.4 6.4 0 0 1-.5 2.5M12 17.5V21M4 4l16 16" />
    </Svg>
  )
}

function SpeakerIcon({ size }: { size?: number }) {
  return (
    <Svg size={size}>
      <path d="M4 9.5h3.5L12 5.5v13l-4.5-4H4Z" />
      <path d="M15.5 9a4 4 0 0 1 0 6M18 6.5a7.5 7.5 0 0 1 0 11" />
    </Svg>
  )
}

function MoreIcon() {
  return (
    <Svg>
      <circle cx="6" cy="12" r="1.3" fill="currentColor" />
      <circle cx="12" cy="12" r="1.3" fill="currentColor" />
      <circle cx="18" cy="12" r="1.3" fill="currentColor" />
    </Svg>
  )
}

function CameraIcon({ size }: { size?: number }) {
  return (
    <Svg size={size}>
      <rect x="3" y="6.5" width="12.5" height="11" rx="2.5" />
      <path d="M15.5 10.5l5-3v9l-5-3" />
    </Svg>
  )
}

function CameraOffIcon({ size }: { size?: number }) {
  return (
    <Svg size={size}>
      <rect x="3" y="6.5" width="12.5" height="11" rx="2.5" />
      <path d="M15.5 10.5l5-3v9l-5-3M3 3.5l17 17" />
    </Svg>
  )
}

function FullscreenIcon() {
  return (
    <Svg size={20}>
      <path d="M4 9V4h5M15 4h5v5M20 15v5h-5M9 20H4v-5" />
    </Svg>
  )
}

function ExitFullscreenIcon() {
  return (
    <Svg size={20}>
      <path d="M9 4v5H4M20 9h-5V4M15 20v-5h5M4 15h5v5" />
    </Svg>
  )
}

function MinimizeIcon() {
  return (
    <Svg>
      <path d="M6 9.5l6 6 6-6" />
    </Svg>
  )
}
