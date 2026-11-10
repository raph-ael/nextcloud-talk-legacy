// System integration for calls that is not part of the call UI: microphone permission,
// keeping the computer awake, network changes, and on macOS the dock, the menu bar icon and
// the incoming-call notification with buttons.
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
      ' braucht Sirplus Talk das Mikrofon. Erlaube es unter Systemeinstellungen › Datenschutz & Sicherheit › Mikrofon.',
    buttons: ['Systemeinstellungen öffnen', 'Abbrechen'],
    defaultId: 0,
    cancelId: 1,
  }
  const { response } = win ? await dialog.showMessageBox(win, options) : await dialog.showMessageBox(options)
  if (response === 0) void shell.openExternal(MIC_SETTINGS)
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

  if (mac) {
    if (prev?.state !== current?.state || prev?.muted !== current?.muted) updateMenus()
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
  return [
    { label: `Anruf: ${c.roomName}`, enabled: false },
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
      { label: 'Sirplus Talk öffnen', click: () => controls?.showWindow() },
      { type: 'separator' },
      { label: 'Sirplus Talk beenden', role: 'quit' },
    ]),
  )
}

/** Call duration next to the menu bar icon, like the green pill of a phone call. */
function updateTimer(): void {
  const startedAt = current?.state === 'connected' ? current.startedAt : null
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
    tray?.setTitle(` ${text}`, { fontType: 'monospacedDigit' })
  }
  tick()
  timer ??= setInterval(tick, 1000)
}

/**
 * Menu bar glyph (speech bubble) as a template image, drawn here so no asset has to be
 * packaged: 16×16 pt with a @2x representation, macOS tints it for light/dark menu bars.
 */
function menuBarImage(): Electron.NativeImage {
  const image = nativeImage.createEmpty()
  for (const scale of [1, 2]) {
    const size = 16 * scale
    const buf = Buffer.alloc(size * size * 4)
    const SS = 4 // supersampling per axis
    // Bubble body (rounded rect) and tail, in points.
    const inBubble = (x: number, y: number): boolean => {
      const l = 1.5, t = 2, r = 14.5, b = 11.5, rad = 3.5
      const cx = Math.min(Math.max(x, l + rad), r - rad)
      const cy = Math.min(Math.max(y, t + rad), b - rad)
      if (x >= l && x <= r && y >= t && y <= b && (x - cx) ** 2 + (y - cy) ** 2 <= rad * rad) return true
      // Tail: triangle (4, 10.5) – (8.5, 10.5) – (4, 14.5)
      return y >= 10.5 && y <= 14.5 && x >= 4 && x <= 8.5 - ((y - 10.5) * 4.5) / 4
    }
    for (let py = 0; py < size; py++) {
      for (let px = 0; px < size; px++) {
        let hits = 0
        for (let sy = 0; sy < SS; sy++) {
          for (let sx = 0; sx < SS; sx++) {
            if (inBubble((px + (sx + 0.5) / SS) / scale, (py + (sy + 0.5) / SS) / scale)) hits++
          }
        }
        buf[(py * size + px) * 4 + 3] = Math.round((255 * hits) / (SS * SS)) // BGRA, black
      }
    }
    image.addRepresentation({ scaleFactor: scale, width: size, height: size, buffer: nativeImage.createFromBitmap(buf, { width: size, height: size, scaleFactor: scale }).toPNG() })
  }
  image.setTemplateImage(true)
  return image
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
    log('resume')
    c.networkChanged()
  })
  powerMonitor.on('suspend', () => log('suspend', current?.state ?? 'no call'))
  if (!mac) return
  tray = new Tray(menuBarImage())
  tray.setToolTip('Sirplus Talk')
  updateMenus()
  app.on('activate', () => c.showWindow())
}
