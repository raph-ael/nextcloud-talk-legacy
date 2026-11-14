// System integration for calls that is not part of the call UI: microphone permission,
// keeping the computer awake, network changes, and on macOS the dock, the menu bar icon and
// the incoming-call notification with buttons, plus the recording state in the menu bar.
import {
  app,
  dialog,
  Menu,
  nativeImage,
  Notification,
  powerMonitor,
  powerSaveBlocker,
  shell,
  systemPreferences,
  Tray,
  type BrowserWindow,
  type MenuItemConstructorOptions,
} from 'electron'
import { networkInterfaces } from 'node:os'
import menuBarIcon from '../../resources/trayTemplate.png?asset'

const mac = process.platform === 'darwin'

/** The part of a `CallView` (docs/calls.md) this module needs. */
export interface SystemCall {
  id: string
  token: string
  roomName: string
  isGroup: boolean
  state: 'dialing' | 'ringing' | 'incoming' | 'connecting' | 'connected' | 'reconnecting' | 'ended'
  callerName: string
  startedAt?: number | null
  muted: boolean
  /** Server-side call recording (docs/calls.md, „Aufnahme“). */
  recording?: 'off' | 'startingAudio' | 'startingVideo' | 'audio' | 'video' | 'failed'
}

/** The part of a `RecordingNotice` this module needs. */
export interface SystemRecordingNotice {
  id: number
  /** Unix seconds (server). */
  timestamp: number
}

/** Call actions of the session, plus the window. */
export interface SystemControls {
  acceptCall(token: string): Promise<void>
  declineCall(token: string): void
  hangUp(): void
  setCallMuted(muted: boolean): void
  networkChanged(): void
  showWindow(): void
  /** Window to attach dialogs to (null while hidden or closed). */
  window(): BrowserWindow | null
}

let controls: SystemControls | null = null
let current: SystemCall | null = null

// --- Microphone -----------------------------------------------------------------------

const MIC_SETTINGS = 'x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone'

/**
 * Makes sure we may use the microphone before a call or a voice message starts. On macOS
 * this asks once (system prompt) and explains a refusal with a link into System Settings.
 * Elsewhere Chromium's own permission handler decides.
 */
export async function ensureMicrophone(purpose: 'call' | 'voice'): Promise<boolean> {
  if (!mac) return true
  // Unpackaged test runs with Chromium's fake microphone need no macOS permission.
  if (!app.isPackaged && app.commandLine.hasSwitch('use-fake-device-for-media-stream')) return true
  const status = systemPreferences.getMediaAccessStatus('microphone')
  if (status === 'granted') return true
  log('microphone', status)
  if (status === 'not-determined' && (await systemPreferences.askForMediaAccess('microphone'))) return true
  const win = controls?.window()
  const options: Electron.MessageBoxOptions = {
    type: 'warning',
    message: 'Kein Zugriff auf das Mikrofon',
    detail:
      (purpose === 'call' ? 'Für Anrufe' : 'Für Sprachnachrichten') +
      ' braucht Wolkenfunk das Mikrofon. Erlaube es unter Systemeinstellungen › Datenschutz & Sicherheit › Mikrofon.',
    buttons: ['Systemeinstellungen öffnen', 'Abbrechen'],
    defaultId: 0,
    cancelId: 1,
  }
  const { response } = win ? await dialog.showMessageBox(win, options) : await dialog.showMessageBox(options)
  if (response === 0) void shell.openExternal(MIC_SETTINGS)
  return false
}

// --- Camera ---------------------------------------------------------------------------

const CAMERA_SETTINGS = 'x-apple.systempreferences:com.apple.preference.security?Privacy_Camera'

/**
 * Makes sure we may use the camera before it is switched on in a call. Same pattern as the
 * microphone: on macOS ask once (system prompt), explain a refusal with a link into System
 * Settings and return false, so the camera stays off. Elsewhere Chromium decides.
 */
export async function ensureCamera(): Promise<boolean> {
  if (!mac) return true
  if (!app.isPackaged && app.commandLine.hasSwitch('use-fake-device-for-media-stream')) return true
  const status = systemPreferences.getMediaAccessStatus('camera')
  if (status === 'granted') return true
  log('camera', status)
  if (status === 'not-determined' && (await systemPreferences.askForMediaAccess('camera'))) return true
  const win = controls?.window()
  const options: Electron.MessageBoxOptions = {
    type: 'warning',
    message: 'Kein Zugriff auf die Kamera',
    detail:
      'Für Videoanrufe braucht Wolkenfunk die Kamera. Erlaube es unter Systemeinstellungen › Datenschutz & Sicherheit › Kamera.',
    buttons: ['Systemeinstellungen öffnen', 'Abbrechen'],
    defaultId: 0,
    cancelId: 1,
  }
  const { response } = win ? await dialog.showMessageBox(win, options) : await dialog.showMessageBox(options)
  if (response === 0) void shell.openExternal(CAMERA_SETTINGS)
  return false
}

// --- Screen recording -----------------------------------------------------------------
const SCREEN_SETTINGS = 'x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture'
let screenAsked = false

/**
 * Makes sure we may record the screen before the picker opens (docs/calls.md
 * „Bildschirmfreigabe“). macOS has no prompt to wait for: the first capture attempt lists Talk
 * under „Aufnahme von Bildschirm & Systemaudio“ (and may show the system's own prompt), a grant
 * only takes effect after a restart. So without the grant: register once, explain with a link
 * into System Settings and the restart, return false. Elsewhere there is nothing to ask.
 */
export async function ensureScreenCapture(register: () => Promise<unknown>): Promise<boolean> {
  if (!mac) return true
  const status = systemPreferences.getMediaAccessStatus('screen')
  if (status === 'granted') return true
  log('screen', status)
  if (!screenAsked) {
    screenAsked = true
    await register().catch(() => undefined)
  }
  const win = controls?.window()
  const options: Electron.MessageBoxOptions = {
    type: 'warning',
    message: 'Bildschirmaufnahme erlauben',
    detail:
      'Zum Teilen des Bildschirms braucht Wolkenfunk die Bildschirmaufnahme. Erlaube sie unter Systemeinstellungen › Datenschutz & Sicherheit › Aufnahme von Bildschirm & Systemaudio und starte Wolkenfunk danach neu.',
    buttons: ['Systemeinstellungen öffnen', 'Abbrechen'],
    defaultId: 0,
    cancelId: 1,
  }
  const { response } = win ? await dialog.showMessageBox(win, options) : await dialog.showMessageBox(options)
  if (response === 0) void shell.openExternal(SCREEN_SETTINGS)
  return false
}

// --- Call state -----------------------------------------------------------------------

/** One line per system action, so device logs show what happened around a call. */
function log(...args: unknown[]): void {
  console.log('[system]', ...args)
}

let awake: number | null = null
let bounceId: number | null = null
let ringNote: Notification | null = null
let ringNoteFor = ''

/** Feed every `callChanged` event in here (main process). */
export function callChanged(call: SystemCall | null): void {
  const prev = current
  current = call && call.state !== 'ended' ? call : null
  const ringing = current?.state === 'incoming'

  // No idle sleep while a call rings or runs; the display may still sleep.
  if (current && awake === null) {
    awake = powerSaveBlocker.start('prevent-app-suspension')
    log('keep awake on')
  } else if (!current && awake !== null) {
    powerSaveBlocker.stop(awake)
    awake = null
    log('keep awake off')
  }

  if (ringing && ringNoteFor !== current!.id) startRinging(current!)
  else if (!ringing && ringNoteFor) stopRinging()
  if (recordingLabel(prev) !== recordingLabel(current)) log('recording', current?.recording ?? 'off')

  if (mac) {
    if (prev?.state !== current?.state || prev?.muted !== current?.muted || recordingLabel(prev) !== recordingLabel(current)) {
      updateMenus()
    }
    updateTimer()
  }
}

function startRinging(call: SystemCall): void {
  stopRinging()
  ringNoteFor = call.id
  // Linux/Windows: flashFrame and a plain notification come from index.ts.
  if (!mac) return
  bounceId = app.dock?.bounce('critical') ?? null
  log('ringing', call.id, 'bounce', bounceId, 'notification', Notification.isSupported())
  if (!Notification.isSupported()) return
  // Silent: the window plays the ringtone. The buttons work while the window is hidden;
  // "Ablehnen" is the close button of the alert.
  const n = new Notification({
    title: call.isGroup ? call.roomName : call.callerName || call.roomName,
    body: call.isGroup ? `Gruppenanruf${call.callerName ? ` von ${call.callerName}` : ''}` : 'Talk-Anruf',
    silent: true,
    actions: [{ type: 'button', text: 'Annehmen' }],
    closeButtonText: 'Ablehnen',
  })
  const token = call.token
  const id = call.id
  const stillRinging = () => current?.id === id && current.state === 'incoming'
  n.on('click', () => controls?.showWindow())
  // macOS refuses notifications that were not allowed (UNErrorDomain 1); the dock and the
  // window still ring.
  n.on('failed', (_e, error) => log('notification failed:', error))
  n.on('action', () => {
    log('notification: accept', id)
    controls?.showWindow()
    if (stillRinging()) void accept(token)
  })
  n.on('close', () => {
    // Closed by the user (not by stopRinging, which forgets the notification first).
    if (ringNote === n && stillRinging()) {
      log('notification: decline', id)
      controls?.declineCall(token)
    }
  })
  ringNote = n
  n.show()
}

function stopRinging(): void {
  if (ringNoteFor) log('ringing stopped', ringNoteFor)
  ringNoteFor = ''
  if (bounceId !== null && bounceId >= 0) app.dock?.cancelBounce(bounceId)
  bounceId = null
  const n = ringNote
  ringNote = null
  n?.close()
}

/** Accept from the notification, the dock or the menu bar. */
async function accept(token: string): Promise<void> {
  if (!(await ensureMicrophone('call'))) return
  await controls?.acceptCall(token).catch((err: Error) => console.warn('accept failed:', err.message))
}

// --- macOS: dock menu, menu bar icon, window ------------------------------------------

let tray: Tray | null = null
let timer: NodeJS.Timeout | null = null

function callItems(): MenuItemConstructorOptions[] {
  const c = current
  if (!c || !controls) return []
  const ctl = controls
  if (c.state === 'incoming') {
    return [
      { label: `Anruf: ${c.isGroup ? c.roomName : c.callerName || c.roomName}`, enabled: false },
      { label: 'Annehmen', click: () => (ctl.showWindow(), void accept(c.token)) },
      { label: 'Ablehnen', click: () => ctl.declineCall(c.token) },
    ]
  }
  const rec = recordingLabel(c)
  return [
    { label: `Anruf: ${c.roomName}`, enabled: false },
    ...(rec ? [{ label: rec, enabled: false }] : []),
    { label: 'Zurück zum Anruf', click: () => ctl.showWindow() },
    { label: 'Stumm', type: 'checkbox', checked: c.muted, click: () => ctl.setCallMuted(!c.muted) },
    { label: 'Auflegen', click: () => ctl.hangUp() },
  ]
}

function updateMenus(): void {
  const calls = callItems()
  app.dock?.setMenu(Menu.buildFromTemplate(calls))
  tray?.setContextMenu(
    Menu.buildFromTemplate([
      ...calls,
      ...(calls.length ? [{ type: 'separator' as const }] : []),
      { label: 'Wolkenfunk öffnen', click: () => controls?.showWindow() },
      { type: 'separator' },
      { label: 'Wolkenfunk beenden', role: 'quit' },
    ]),
  )
}

/** Menu line for the recording state (null: nothing to show), as in docs/calls.md. */
function recordingLabel(c: SystemCall | null): string | null {
  switch (c?.recording) {
    case 'audio':
    case 'video':
      return '● Aufnahme läuft'
    case 'startingAudio':
    case 'startingVideo':
      return 'Aufnahme startet …'
    default:
      return null
  }
}

/**
 * Call duration next to the menu bar icon, like the green pill of a phone call; "● REC"
 * in front while the call is recorded (the menu bar is monochrome, the dot carries it).
 */
function updateTimer(): void {
  const startedAt = current?.state === 'connected' || current?.state === 'reconnecting' ? current.startedAt : null
  if (!startedAt) {
    if (timer) clearInterval(timer)
    timer = null
    tray?.setTitle('')
    return
  }
  const tick = () => {
    const s = Math.max(0, Math.floor((Date.now() - startedAt) / 1000))
    const p = (n: number) => String(n).padStart(2, '0')
    const text = s >= 3600 ? `${Math.floor(s / 3600)}:${p(Math.floor(s / 60) % 60)}:${p(s % 60)}` : `${Math.floor(s / 60)}:${p(s % 60)}`
    const rec = current?.recording === 'audio' || current?.recording === 'video' ? ' ● REC' : ''
    tray?.setTitle(`${rec} ${text}`, { fontType: 'monospacedDigit' })
  }
  tick()
  timer ??= setInterval(tick, 1000)
}

/**
 * Menu bar symbol: a template image (16×16 pt, @2x next to it), so macOS tints it for light and
 * dark menu bars. Generated by design/icon/build-icons.py.
 */
function menuBarImage(): Electron.NativeImage {
  const image = nativeImage.createFromPath(menuBarIcon)
  image.setTemplateImage(true)
  return image
}

// --- Recording notices ----------------------------------------------------------------

const knownNotices = new Set<number>()
/** Notices from before this run (a minute of clock skew allowed) are old news. */
const startedAt = Date.now() / 1000 - 60

/**
 * Feed every `recordingsChanged` event in here (main process). The quiet system notification
 * comes from index.ts on every platform; on macOS a new notice also bounces the dock once
 * while the app is in the background (informational: one hop, not until clicked).
 */
export function recordingsChanged(items: SystemRecordingNotice[]): void {
  // The core only reports changes, so the first list may already hold the new notice.
  const fresh = items.filter((i) => !knownNotices.has(i.id) && i.timestamp >= startedAt)
  for (const i of items) knownNotices.add(i.id)
  if (!fresh.length) return
  const bounce = mac && !controls?.window()?.isFocused() ? app.dock?.bounce('informational') : null
  log('recording notices', fresh.map((i) => i.id).join(','), 'bounce', bounce ?? 'no')
}

// --- Network --------------------------------------------------------------------------

/**
 * Electron has no "network changed" event: compare the addresses every few seconds. IPv6
 * counts per interface only (privacy addresses rotate without a network change).
 */
function networkSignature(): string {
  const parts: string[] = []
  for (const [name, addrs] of Object.entries(networkInterfaces())) {
    for (const a of addrs ?? []) {
      if (a.internal) continue
      if (a.family === 'IPv4') parts.push(`${name}=${a.address}`)
      else if (!a.address.startsWith('fe80:')) parts.push(`${name}=v6`)
    }
  }
  return [...new Set(parts)].sort().join(',')
}

function watchNetwork(onChange: () => void): void {
  let last = networkSignature()
  let pending: NodeJS.Timeout | null = null
  setInterval(() => {
    const now = networkSignature()
    if (now === last) return
    last = now
    // Interfaces often change in steps (address gone, new one a moment later).
    if (pending) clearTimeout(pending)
    pending = setTimeout(() => {
      pending = null
      log('network changed')
      onChange()
    }, 1500)
  }, 3000).unref()
}

// --- Setup ----------------------------------------------------------------------------

/** Once after `app.whenReady()`. */
export function installSystem(c: SystemControls): void {
  controls = c
  log('ready', process.platform, mac ? `microphone ${systemPreferences.getMediaAccessStatus('microphone')}` : '')
  watchNetwork(() => c.networkChanged())
  // After sleep the sockets are dead; also covers waking with another network.
  powerMonitor.on('resume', () => {
    log('resume', current?.state ?? 'no call', current?.recording ?? '')
    c.networkChanged()
  })
  // A recording runs on the server and goes on while we sleep; after waking the call
  // reconnects and the recording state comes back from the server.
  powerMonitor.on('suspend', () => log('suspend', current?.state ?? 'no call', current?.recording ?? ''))
  app.on('before-quit', () => log('quit', current?.state ?? 'no call'))
  if (!mac) return
  tray = new Tray(menuBarImage())
  tray.setToolTip('Wolkenfunk')
  updateMenus()
  app.on('activate', () => c.showWindow())
}
