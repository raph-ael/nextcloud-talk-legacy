// Device lists, microphone level and camera preview, shared by the menus at „Stumm“ and
// „Kamera“ in the call (docs/calls.md, „Desktop-Geräte wie bei Meet“) and the settings page
// „Audio und Video“ (docs/settings.md). The logic is here once; the look differs: the menus
// are dark, the page (`page`) wears the app's design.
//
// No device goes on by itself: outside a call the microphone and the camera open only on a
// click, and every opened track is stopped when the test ends or its component goes.

import { useCallback, useEffect, useRef, useState, useSyncExternalStore, type ReactNode, type RefObject } from 'react'
import { useCall } from './store'
import {
  cameraText,
  currentTrack,
  listDevices,
  mediaState,
  micText,
  openCameraPreview,
  openMicrophone,
  subscribeMedia,
  type Devices,
  type MediaState,
} from './devices'

export function useMedia(): MediaState {
  return useSyncExternalStore(subscribeMedia, mediaState)
}

// --- Lists ------------------------------------------------------------------------------

const reloaders = new Set<() => void>()

/** The devices of the system; the lists follow plugging in and out. `null` until known. */
export function useDevices(): Devices | null {
  const [devices, setDevices] = useState<Devices | null>(null)
  useEffect(() => {
    let live = true
    const load = () =>
      void listDevices()
        .then((d) => live && setDevices(d))
        .catch(() => undefined)
    load()
    reloaders.add(load)
    navigator.mediaDevices.addEventListener('devicechange', load)
    return () => {
      live = false
      reloaders.delete(load)
      navigator.mediaDevices.removeEventListener('devicechange', load)
    }
  }, [])
  return devices
}

export interface DeviceOption {
  /** '' is the system default. */
  id: string
  label: string
}

/** „Systemstandard“, then the devices; one without a name is „Gerät n“. */
export function deviceOptions(list: MediaDeviceInfo[] | undefined): DeviceOption[] {
  return [{ id: '', label: 'Systemstandard' }, ...(list ?? []).map((d, i) => ({ id: d.deviceId, label: d.label || `Gerät ${i + 1}` }))]
}

// --- A device opened for a test ---------------------------------------------------------

interface DeviceTest {
  /** The open device; null before, after and while it opens. */
  stream: MediaStream | null
  /** Asked for and not ended (opening or open). */
  wanted: boolean
  /** Why it did not open, in the words of the call. */
  error: string | null
  start: () => void
  stop: () => void
}

function stopStream(stream: MediaStream): void {
  stream.getTracks().forEach((t) => t.stop())
}

/**
 * Opens the chosen microphone or camera on request, through the serial queue of the calls.
 * A new choice reopens with the new device. `blocked` (a call took over, the page is
 * covered) ends the test. Every track is stopped at the end and when the component goes.
 */
function useDeviceTest(kind: 'microphone' | 'camera', deviceId: string, blocked: boolean): DeviceTest {
  const [wanted, setWanted] = useState(false)
  const [stream, setStream] = useState<MediaStream | null>(null)
  const [error, setError] = useState<string | null>(null)
  useEffect(() => {
    if (blocked) setWanted(false)
  }, [blocked])
  useEffect(() => {
    if (!wanted || blocked) {
      setStream(null)
      return
    }
    let live = true
    let opened: MediaStream | null = null
    void (async () => {
      try {
        // macOS asks for its permission first; refused, its dialog has explained it.
        if (!(await window.talk.ensureMedia(kind))) {
          if (live) setWanted(false)
          return
        }
        if (!live) return
        const s = await (kind === 'microphone' ? openMicrophone() : openCameraPreview(deviceId))
        if (!live) {
          stopStream(s)
          return
        }
        opened = s
        // Unplugged in the middle of the test: it ends, the button is back.
        s.getTracks()[0]?.addEventListener('ended', () => live && setWanted(false))
        setStream(s)
        // The names of the devices may only be known once one was opened.
        reloaders.forEach((load) => load())
      } catch (err) {
        if (!live) return
        setError(kind === 'microphone' ? micText(err) : cameraText(err))
        setWanted(false)
      }
    })()
    return () => {
      live = false
      if (opened) stopStream(opened)
    }
  }, [wanted, blocked, deviceId, kind])
  const start = useCallback(() => {
    setError(null)
    setWanted(true)
  }, [])
  const stop = useCallback(() => setWanted(false), [])
  return { stream, wanted, error, start, stop }
}

// --- Microphone level -------------------------------------------------------------------

/** Moves the returned bar with the level of `track` (empty without one and while it is silent). */
function useLevel(track: MediaStreamTrack | null): RefObject<HTMLDivElement | null> {
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
      if (bar.current) bar.current.style.transform = ''
    }
  }, [track])
  return bar
}

/**
 * In a call: the level of the microphone the call sends (empty while muted). Outside a call
 * (only on the page): „Mikrofon testen“ opens the chosen microphone until „Test beenden“.
 * A call that begins ends the test, and the bar shows the call's microphone.
 */
export function MicLevel({ page = false, covered = false }: { page?: boolean; covered?: boolean }) {
  const media = useMedia()
  const muted = useCall((s) => s.call?.muted ?? false)
  const test = useDeviceTest('microphone', media.microphoneId, media.inCall || covered)
  const track = media.inCall ? currentTrack('audio') : (test.stream?.getAudioTracks()[0] ?? null)
  const bar = useLevel(track)
  if (!page) {
    if (!track) return null
    return (
      <div className="call-meter" aria-hidden>
        <div className="call-meter-bar" ref={bar} />
      </div>
    )
  }
  const shown = media.inCall || !!test.stream
  return (
    <div className="media-test">
      {shown && (
        <div className="media-meter" aria-hidden>
          <div className="media-meter-bar" ref={bar} />
        </div>
      )}
      {shown && (
        <p className="media-hint">
          {media.inCall && muted
            ? 'Stummgeschaltet'
            : media.inCall && media.micError
              ? media.micError.text
              : 'Sprich etwas, der Balken zeigt deinen Pegel.'}
        </p>
      )}
      {!media.inCall && (
        <button className="pill-btn" onClick={test.stream ? test.stop : test.start}>
          {test.stream ? 'Test beenden' : 'Mikrofon testen'}
        </button>
      )}
      {!media.inCall && test.error && (
        <p className="media-error" role="alert">
          {test.error}
        </p>
      )}
    </div>
  )
}

// --- Camera preview ---------------------------------------------------------------------

function useVideo(stream: MediaStream | null): RefObject<HTMLVideoElement | null> {
  const ref = useRef<HTMLVideoElement>(null)
  useEffect(() => {
    const v = ref.current
    if (!v || !stream) return
    v.srcObject = stream
    void v.play().catch(() => undefined)
    return () => {
      v.srcObject = null
    }
  }, [stream])
  return ref
}

/**
 * The running camera of the call; while it is off a preview only on request (the camera
 * light does not go on just because a menu or the page opened). On the page the preview
 * ends with „Vorschau beenden“, in a menu with the menu.
 */
export function CameraPreview({ page = false, covered = false, icon }: { page?: boolean; covered?: boolean; icon?: ReactNode }) {
  const media = useMedia()
  const running = media.camera
  const test = useDeviceTest('camera', media.cameraId, !!running || covered)
  const stream = running ?? test.stream
  const ref = useVideo(stream)
  const video = stream && (
    <video
      ref={ref}
      className={`${page ? 'media-preview' : 'call-menu-preview'} mirrored`}
      aria-label="Vorschau der Kamera"
      autoPlay
      muted
      playsInline
    />
  )
  if (page) {
    return (
      <div className="media-test">
        {video}
        {!running && (
          <button className="pill-btn" onClick={test.stream ? test.stop : test.start}>
            {test.stream ? 'Vorschau beenden' : 'Vorschau anzeigen'}
          </button>
        )}
        {!running && test.error && (
          <p className="media-error" role="alert">
            {test.error}
          </p>
        )}
      </div>
    )
  }
  if (video) return video
  return (
    <>
      {test.error && <div className="call-menu-error">{test.error}</div>}
      <button className="call-menu-action" onClick={test.start} disabled={test.wanted}>
        {icon}
        Vorschau anzeigen
      </button>
    </>
  )
}
