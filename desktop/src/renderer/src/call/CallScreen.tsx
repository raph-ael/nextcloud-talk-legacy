// Call UI in the main window, after Signal Desktop: incoming-call bar on top, full call view
// (1:1 big avatar, group tiles), control bar, "back to call" bar while minimized.

import { useEffect, useRef, useState, useSyncExternalStore, type ReactNode } from 'react'
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
  toggleMute,
  useCall,
} from './store'
import { chooseMicrophone, chooseSpeaker, listDevices, mediaState, subscribeMedia, type AudioDevices } from './engine'

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
  return (
    <div className={`call-screen${ended ? ' ended' : ''}`} role="dialog" aria-label={`Anruf mit ${call.roomName}`}>
      <div
        className="call-backdrop"
        style={{ backgroundImage: `url("${window.talk.avatarUrl(call.token, room?.avatarVersion ?? '')}")` }}
      />
      <header className="call-top">
        <button className="call-icon-btn" onClick={() => minimize(true)} title="Zu den Chats" aria-label="Anruf verkleinern">
          <MinimizeIcon />
        </button>
        {tiles && (
          <div className="call-top-title">
            <strong className="ellipsis">{call.roomName}</strong>
            <CallStatusLine call={call} />
          </div>
        )}
        {!ended && <RecordingIndicator call={call} />}
      </header>

      <main className="call-stage">
        {tiles ? (
          <div className={`call-tiles n${Math.min(call.participants.length, 9)}`}>
            {call.participants.map((p) => (
              <Tile key={p.sessionId} p={p} />
            ))}
          </div>
        ) : (
          <div className="call-solo">
            <Avatar name={call.roomName} token={call.token} version={room?.avatarVersion} size={112} />
            <h1 className="ellipsis">{call.roomName}</h1>
            <CallStatusLine call={call} />
          </div>
        )}
        {media.micError && !ended && <div className="call-notice">{media.micError}</div>}
      </main>

      <footer className="call-controls">
        <DevicesButton disabled={ended} />
        {/* Reserved for the camera button once video exists. */}
        <span className="call-slot" aria-hidden />
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
  return (
    <div className={`call-tile${p.speaking && !p.muted ? ' speaking' : ''}${p.connected ? '' : ' waiting'}`}>
      <Avatar name={p.name} userId={p.userId || undefined} size={72} />
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
        title="Mikrofon und Lautsprecher"
        aria-label="Mikrofon und Lautsprecher"
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
  const [devices, setDevices] = useState<AudioDevices | null>(null)
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
  const section = (title: string, list: MediaDeviceInfo[] | undefined, current: string, choose: (id: string) => void) => (
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
    </div>
  )
  return (
    <div className="call-menu" role="menu">
      {section('Mikrofon', devices?.microphones, media.microphoneId, chooseMicrophone)}
      {section('Lautsprecher', devices?.speakers, media.speakerId, chooseSpeaker)}
    </div>
  )
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

function MinimizeIcon() {
  return (
    <Svg>
      <path d="M6 9.5l6 6 6-6" />
    </Svg>
  )
}
