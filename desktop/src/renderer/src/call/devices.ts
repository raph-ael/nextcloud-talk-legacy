// Microphone, camera and speaker of calls, like Google Meet (docs/calls.md, „Desktop-Geräte
// wie bei Meet“): every getUserMedia goes through one serial queue, the system default is
// followed with a notice, unplugged devices fall back and come back, ended tracks are reopened
// with backoff. The engine (engine.ts) only puts the tracks on its senders.

const MIC_KEY = 'callMicrophone'
const SPEAKER_KEY = 'callSpeaker'
const CAMERA_KEY = 'callCamera'
/** Reopening an ended track: waits before the attempts, then the notice with „Erneut versuchen“. */
const BACKOFF_MS = [500, 1000, 2000]
/** An unplugged camera that comes back within this time is switched on again. */
const CAMERA_RETURN_MS = 60_000
/** Same values as the core's speaking indicator. */
const SPEAKING_LEVEL = 0.04
const SPEAKING_HOLD_MS = 700

export type DeviceKind = 'audio' | 'video'

export interface DeviceProblem {
  text: string
  /** „Erneut versuchen“ is offered. */
  retry: boolean
}

export interface MediaState {
  /** Why there is no microphone (shown in the call view); null when it works. */
  micError: DeviceProblem | null
  /** Why the camera could not be (re)opened, with „Erneut versuchen“; null otherwise. */
  camError: DeviceProblem | null
  /** Chosen devices; '' follows the system default. */
  microphoneId: string
  speakerId: string
  cameraId: string
  /** Our camera while it is on (self preview). */
  camera: MediaStream | null
  /** We are speaking (publisher level): frame around the self preview. */
  selfSpeaking: boolean
}

let ms: MediaState = {
  micError: null,
  camError: null,
  microphoneId: stored(MIC_KEY),
  speakerId: stored(SPEAKER_KEY),
  cameraId: stored(CAMERA_KEY),
  camera: null,
  selfSpeaking: false,
}
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

// --- Hooks to the engine and the call UI ------------------------------------------------

type TrackSink = (kind: DeviceKind, track: MediaStreamTrack | null) => void
let trackSink: TrackSink = () => undefined
let notify: (text: string) => void = (text) => console.info('[devices]', text)

/** The engine puts every new microphone/camera track (or null) on its senders. */
export function setTrackSink(sink: TrackSink): void {
  trackSink = sink
}

/** Short notices („Mikrofon: …“) go to the call toast. */
export function setDeviceNotice(fn: (text: string) => void): void {
  notify = fn
}

// --- Serial queue -----------------------------------------------------------------------

let queue: Promise<unknown> = Promise.resolve()

/**
 * Runs device operations one after another: two overlapping getUserMedia calls (a pick
 * during a reopen, a preview during a switch) would race for the device and the sender.
 * An operation must not wait for another serial one (it would wait for itself).
 */
export function serial<T>(op: () => Promise<T>): Promise<T> {
  const run = queue.then(op, op)
  queue = run.catch(() => undefined)
  return run
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms))

function errName(err: unknown): string {
  return (err as DOMException)?.name ?? ''
}

// --- Microphone -------------------------------------------------------------------------

/** startAudio came and no stopAudio yet. */
let inCall = false
let mic: MediaStreamTrack | null = null
let micEnabled = true
/** Bumped by every new microphone request: older ones give up. */
let micGen = 0
/** The user picked the device: no „Mikrofon: …“ notice for the result. */
let micPicked = false
/** Label of the microphone used last in this call (also after it ended). */
let micLabel: string | null = null

export function microphoneConstraints(deviceId = ms.microphoneId): MediaTrackConstraints {
  return {
    echoCancellation: true,
    noiseSuppression: true,
    autoGainControl: true,
    ...(deviceId ? { deviceId: { ideal: deviceId } } : {}),
  }
}

export function currentTrack(kind: DeviceKind): MediaStreamTrack | null {
  return kind === 'audio' ? mic : cam
}

export function micText(err: unknown): string {
  switch (errName(err)) {
    case 'NotAllowedError':
    case 'SecurityError':
      return 'Kein Zugriff auf das Mikrofon'
    case 'NotFoundError':
    case 'OverconstrainedError':
      return 'Kein Mikrofon gefunden'
    case 'NotReadableError':
      return 'Das Mikrofon wird von einer anderen App benutzt'
    default:
      return `Mikrofon nicht verfügbar (${(err as Error)?.message ?? err})`
  }
}

/** One attempt; true when done (opened, or no longer wanted). */
function openMic(gen: number): Promise<{ ok: true } | { ok: false; err: unknown }> {
  return serial(async () => {
    if (!inCall || gen !== micGen) return { ok: true as const }
    let track: MediaStreamTrack | undefined
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: microphoneConstraints(), video: false })
      track = stream.getAudioTracks()[0]
      if (!track) throw new DOMException('Kein Mikrofon gefunden', 'NotFoundError')
    } catch (err) {
      return { ok: false as const, err }
    }
    if (!inCall || gen !== micGen) {
      track.stop()
      return { ok: true as const }
    }
    useMic(track)
    return { ok: true as const }
  })
}

/**
 * Opens the microphone (the chosen one, else the default). `backoff`: a track ended or the
 * device is busy, try again after 0.5/1/2 s before giving up.
 */
async function acquireMic(backoff: boolean): Promise<void> {
  const gen = ++micGen
  let r = await openMic(gen)
  for (const wait of backoff ? BACKOFF_MS : []) {
    if (r.ok || gen !== micGen) break
    // Permission and „no microphone“ do not get better by waiting (a plug-in comes as devicechange).
    if (['NotAllowedError', 'SecurityError', 'NotFoundError'].includes(errName(r.err))) break
    await sleep(wait)
    r = await openMic(gen)
  }
  if (r.ok || gen !== micGen || !inCall) return
  console.warn('microphone:', r.err)
  setMedia({ micError: { text: micText(r.err), retry: errName(r.err) !== 'NotAllowedError' } })
}

function useMic(track: MediaStreamTrack): void {
  const old = mic
  mic = track
  track.enabled = micEnabled
  track.onended = () => {
    // Unplugged, sleep, driver reset: back to the chosen one or the default.
    if (mic !== track || !inCall) return
    mic = null
    trackSink('audio', null)
    void acquireMic(true)
  }
  trackSink('audio', track)
  old?.stop()
  setMedia({ micError: null })
  // Another device than before without a pick of the user: say which one is used now.
  if (!micPicked && micLabel !== null && micLabel !== track.label) notify(`Mikrofon: ${track.label || 'Standard'}`)
  micLabel = track.label
  micPicked = false
}

export function setMicrophoneEnabled(enabled: boolean): void {
  micEnabled = enabled
  if (mic) mic.enabled = enabled
}

export function retryMicrophone(): void {
  setMedia({ micError: null })
  void acquireMic(true)
}

/** '' is the system default. Switches a running call at once (no renegotiation). */
export function chooseMicrophone(deviceId: string): void {
  store(MIC_KEY, deviceId)
  setMedia({ microphoneId: deviceId })
  if (!inCall) return
  micPicked = true
  void (async () => {
    const gen = ++micGen
    const r = await openMic(gen)
    if (!r.ok && gen === micGen) {
      micPicked = false
      console.warn('switching microphone failed:', r.err)
      // The old microphone keeps working (if there is one).
      if (mic) notify(`Mikrofon konnte nicht gewechselt werden: ${micText(r.err)}`)
      else setMedia({ micError: { text: micText(r.err), retry: true } })
    }
  })()
}

// --- Call lifecycle ---------------------------------------------------------------------

export function callStarted(): void {
  inCall = true
  void acquireMic(false)
  void applySpeaker(false)
}

/** stopAudio: microphone and camera go, the LEDs go off. */
export function callStopped(): void {
  inCall = false
  micGen++
  const old = mic
  mic = null
  micLabel = null
  trackSink('audio', null)
  old?.stop()
  // A camera request on the way must not open it after the call.
  camWanted = false
  camOpening = false
  camGen++
  stopCamera()
  cameraLost = null
  setMedia({ micError: null, camError: null, selfSpeaking: false })
}

// --- Camera -----------------------------------------------------------------------------

/** The core wants the camera on (setCameraEnabled). */
let camWanted = false
let cam: MediaStreamTrack | null = null
let camGen = 0
/** An open request is on the way (repeated commands of the core just wait for it). */
let camOpening = false
/** Unplugged while on: switched on again when the device comes back within 60 s. */
let cameraLost: { deviceId: string; until: number } | null = null

function camConstraints(deviceId = ms.cameraId): MediaTrackConstraints {
  return {
    ...(deviceId ? { deviceId: { ideal: deviceId } } : {}),
    width: { ideal: 1280 },
    height: { ideal: 720 },
    frameRate: { ideal: 30 },
  }
}

export function cameraText(err: unknown): string {
  switch (errName(err)) {
    case 'NotAllowedError':
    case 'SecurityError':
      return 'Kein Zugriff auf die Kamera'
    case 'NotFoundError':
    case 'OverconstrainedError':
      return 'Keine Kamera gefunden'
    case 'NotReadableError':
    case 'AbortError':
      return 'Die Kamera wird von einer anderen App benutzt'
    default:
      return `Kamera nicht verfügbar (${(err as Error)?.message ?? err})`
  }
}

function openCam(gen: number, deviceId = ms.cameraId): Promise<{ ok: true } | { ok: false; err: unknown }> {
  return serial(async () => {
    if (!camWanted || gen !== camGen) return { ok: true as const }
    let track: MediaStreamTrack | undefined
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: false, video: camConstraints(deviceId) })
      track = stream.getVideoTracks()[0]
      if (!track) throw new DOMException('Keine Kamera gefunden', 'NotFoundError')
    } catch (err) {
      return { ok: false as const, err }
    }
    if (!camWanted || gen !== camGen) {
      track.stop()
      return { ok: true as const }
    }
    useCam(track)
    return { ok: true as const }
  })
}

/** Gives up on the camera: the core sets `cameraOn` back and tells the others. */
function cameraFailed(problem: DeviceProblem | null, toast: string | null): void {
  camWanted = false
  camOpening = false
  camGen++
  stopCamera()
  window.talk.mediaReport({ type: 'cameraState', on: false })
  if (problem) setMedia({ camError: problem })
  if (toast) notify(toast)
}

async function acquireCam(backoff: boolean): Promise<void> {
  const gen = ++camGen
  camOpening = true
  try {
    await acquireCamRun(gen, backoff)
  } finally {
    if (gen === camGen) camOpening = false
  }
}

async function acquireCamRun(gen: number, backoff: boolean): Promise<void> {
  let r = await openCam(gen)
  for (const wait of backoff ? BACKOFF_MS : []) {
    if (r.ok || gen !== camGen) break
    if (['NotAllowedError', 'SecurityError', 'NotFoundError'].includes(errName(r.err))) break
    await sleep(wait)
    r = await openCam(gen)
  }
  if (r.ok || gen !== camGen) return
  console.warn('camera:', r.err)
  const name = errName(r.err)
  // Busy or broken: offer „Erneut versuchen“; no camera / no permission: just say so.
  if (['NotAllowedError', 'SecurityError', 'NotFoundError', 'OverconstrainedError'].includes(name)) cameraFailed(null, cameraText(r.err))
  else cameraFailed({ text: cameraText(r.err), retry: true }, null)
}

function useCam(track: MediaStreamTrack): void {
  const old = cam
  cam = track
  track.contentHint = 'motion'
  track.onended = () => void camEnded(track)
  trackSink('video', track)
  old?.stop()
  setMedia({ camera: new MediaStream([track]), camError: null })
}

function stopCamera(): void {
  const old = cam
  cam = null
  if (old) {
    trackSink('video', null)
    old.stop()
  }
  if (ms.camera) setMedia({ camera: null })
}

/** The camera track ended: unplugged (off, back within 60 s) or a hiccup (reopen with backoff). */
async function camEnded(track: MediaStreamTrack): Promise<void> {
  if (cam !== track || !camWanted) return
  const deviceId = track.getSettings().deviceId ?? ''
  cam = null
  trackSink('video', null)
  const gone = await serial(async () => {
    const all = await navigator.mediaDevices.enumerateDevices().catch(() => [] as MediaDeviceInfo[])
    return !all.some((d) => d.kind === 'videoinput' && d.deviceId === deviceId)
  })
  if (!camWanted || cam) return
  if (gone) {
    cameraFailed(null, 'Kamera getrennt')
    cameraLost = { deviceId, until: Date.now() + CAMERA_RETURN_MS }
  } else {
    void acquireCam(true)
  }
}

/** MediaEngine `setCameraEnabled`: on opens (or just keeps) the camera, off stops it (LED off). */
export function setCameraEnabled(enabled: boolean): void {
  if (!enabled) {
    camWanted = false
    camOpening = false
    camGen++
    cameraLost = null
    stopCamera()
    if (ms.camError) setMedia({ camError: null })
    return
  }
  cameraLost = null
  if (ms.camError) setMedia({ camError: null })
  if (camWanted && cam && cam.readyState === 'live') {
    // A new publisher/mesh peer: the engine attaches the open track by itself.
    trackSink('video', cam)
    return
  }
  // Still opening: the new peer gets the track when it is there.
  if (camWanted && camOpening) return
  camWanted = true
  void acquireCam(false)
}

/** „Erneut versuchen“: through the core, so the others learn about it. */
export function retryCamera(): void {
  setMedia({ camError: null })
  void window.talk.setCameraEnabled(true)
}

export function chooseCamera(deviceId: string): void {
  store(CAMERA_KEY, deviceId)
  setMedia({ cameraId: deviceId })
  if (!camWanted || !cam) return
  void (async () => {
    const gen = ++camGen
    const r = await openCam(gen, deviceId)
    if (!r.ok && gen === camGen) {
      console.warn('switching camera failed:', r.err)
      // The old camera keeps running (its track is still live).
      notify(`Kamera konnte nicht gewechselt werden: ${cameraText(r.err)}`)
    }
  })()
}

// --- Speaker ----------------------------------------------------------------------------

/** Call audio elements; the engine registers them, they always play on `sinkId`. */
const outputs = new Set<HTMLMediaElement>()
/** The device the call plays on now (resolved: the chosen one, else the current default). */
let sinkId = ''
let sinkLabel: string | null = null

export function addOutput(el: HTMLMediaElement): void {
  outputs.add(el)
  void applySink(el, sinkId)
}

export function removeOutput(el: HTMLMediaElement): void {
  outputs.delete(el)
}

/** Output device for call audio and call tones ('' = system default). */
export function speakerId(): string {
  return sinkId
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

/**
 * Chromium keeps playing on the device a stream started on, also when the system default
 * moves (headset plugged in). Following the default therefore means: play on the concrete
 * device that is the default right now, re-resolved on every device change.
 */
async function applySpeaker(announce: boolean, all?: MediaDeviceInfo[]): Promise<void> {
  const list = all ?? (await navigator.mediaDevices.enumerateDevices().catch(() => [] as MediaDeviceInfo[]))
  const outs = list.filter((d) => d.kind === 'audiooutput')
  let target: MediaDeviceInfo | undefined
  if (ms.speakerId) target = outs.find((d) => d.deviceId === ms.speakerId)
  if (!target) {
    const def = outs.find((d) => d.deviceId === 'default')
    target = (def?.groupId && outs.find((d) => isReal(d) && d.groupId === def.groupId)) || def
  }
  const id = target && isReal(target) ? target.deviceId : ''
  const label = target ? cleanLabel(target.label) : ''
  const changed = id !== sinkId
  sinkId = id
  for (const el of outputs) void applySink(el, id)
  if (announce && inCall && changed && sinkLabel !== null && label && label !== sinkLabel) notify(`Lautsprecher: ${label}`)
  sinkLabel = label
}

/** „Standard - Foo“ (Chromium's default entry) → „Foo“. */
function cleanLabel(label: string): string {
  return label.replace(/^(Standard|Default|Kommunikation|Communications)\s+-\s+/, '')
}

export function chooseSpeaker(deviceId: string): void {
  store(SPEAKER_KEY, deviceId)
  setMedia({ speakerId: deviceId })
  void applySpeaker(false)
}

// --- Device changes ---------------------------------------------------------------------

let deviceTimer: ReturnType<typeof setTimeout> | null = null
let watching = false

export function watchDevices(): void {
  if (watching) return
  watching = true
  // Bluetooth devices (AirPods) come and go in several steps: act once it is quiet.
  navigator.mediaDevices.addEventListener('devicechange', () => {
    if (deviceTimer) clearTimeout(deviceTimer)
    deviceTimer = setTimeout(() => {
      deviceTimer = null
      void devicesChanged()
    }, 600)
  })
}

/** Devices came or went, or the system default changed. */
async function devicesChanged(): Promise<void> {
  const all = await serial(() => navigator.mediaDevices.enumerateDevices()).catch(() => null)
  if (!all) return
  await applySpeaker(true, all)
  if (!inCall) return

  // Microphone.
  const inputs = all.filter((d) => d.kind === 'audioinput')
  if (!mic || mic.readyState === 'ended') {
    // Plugged in after none worked (a running backoff is replaced).
    if (ms.micError || !mic) void acquireMic(false)
  } else {
    const live = mic.getSettings()
    const picked = ms.microphoneId
    if (picked && inputs.some((d) => d.deviceId === picked)) {
      if (live.deviceId !== picked) void acquireMic(false) // the chosen one is back
    } else if (picked && live.deviceId === picked) {
      void acquireMic(false) // the chosen one is gone: the default
    } else {
      // Following the default: macOS, Windows and PulseAudio list it as 'default' with the
      // real device's groupId; another groupId than the live track means the default moved.
      const def = inputs.find((d) => d.deviceId === 'default')
      if (def?.groupId && live.groupId && def.groupId !== live.groupId) void acquireMic(false)
    }
  }

  // Camera: an unplugged one came back.
  if (cameraLost) {
    if (Date.now() > cameraLost.until) cameraLost = null
    else if (all.some((d) => d.kind === 'videoinput' && d.deviceId === cameraLost!.deviceId)) {
      cameraLost = null
      notify('Kamera wieder verbunden')
      void window.talk.setCameraEnabled(true)
    }
  } else if (camWanted && cam && ms.cameraId) {
    // The chosen camera came back while another one runs.
    const live = cam.getSettings().deviceId
    if (live !== ms.cameraId && all.some((d) => d.kind === 'videoinput' && d.deviceId === ms.cameraId)) void acquireCam(false)
  }
}

// --- Lists, previews --------------------------------------------------------------------

export interface Devices {
  microphones: MediaDeviceInfo[]
  speakers: MediaDeviceInfo[]
  cameras: MediaDeviceInfo[]
}

function isReal(d: MediaDeviceInfo): boolean {
  return d.deviceId !== 'default' && d.deviceId !== 'communications' && d.deviceId !== ''
}

export async function listDevices(): Promise<Devices> {
  const all = await navigator.mediaDevices.enumerateDevices()
  return {
    microphones: all.filter((d) => d.kind === 'audioinput' && isReal(d)),
    speakers: all.filter((d) => d.kind === 'audiooutput' && isReal(d)),
    cameras: all.filter((d) => d.kind === 'videoinput' && d.deviceId !== ''),
  }
}

/** Camera preview in the device menu while our camera is off; the caller stops it. */
export function openCameraPreview(deviceId: string): Promise<MediaStream> {
  return serial(() =>
    navigator.mediaDevices.getUserMedia({
      audio: false,
      video: { ...(deviceId ? { deviceId: { ideal: deviceId } } : {}), width: { ideal: 320 }, height: { ideal: 180 } },
    }),
  )
}

/** A recording microphone outside calls (voice messages): the chosen device, in the queue. */
export function openMicrophone(extra: MediaTrackConstraints = {}): Promise<MediaStream> {
  return serial(() => navigator.mediaDevices.getUserMedia({ audio: { ...microphoneConstraints(), ...extra }, video: false }))
}

// --- Own speaking frame -----------------------------------------------------------------

let speakingTimer: ReturnType<typeof setTimeout> | null = null

/** Publisher level (our microphone as sent), from the engine's level reports. */
export function selfLevel(level: number): void {
  if (level < SPEAKING_LEVEL || !micEnabled) return
  if (!ms.selfSpeaking) setMedia({ selfSpeaking: true })
  if (speakingTimer) clearTimeout(speakingTimer)
  speakingTimer = setTimeout(() => setMedia({ selfSpeaking: false }), SPEAKING_HOLD_MS)
}

// --- Diagnostics ------------------------------------------------------------------------

export function deviceSnapshot(): Record<string, unknown> {
  return {
    mic: mic ? { label: mic.label, enabled: mic.enabled, state: mic.readyState } : null,
    micError: ms.micError,
    camera: cam ? { label: cam.label, state: cam.readyState, settings: cam.getSettings() } : null,
    camWanted,
    camError: ms.camError,
    cameraLost,
    sinkId,
    sinkLabel,
  }
}
