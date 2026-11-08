import { app, BrowserWindow, ipcMain, nativeTheme, Notification, protocol, safeStorage, shell } from 'electron'
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { withAttachment, type AccountInfo, type AppEvent, type CoreEvent, type RoomView, type TalkEvent } from '@shared/core'

// --- Native core -------------------------------------------------------------------

interface NativeLoginFlow {
  readonly loginUrl: string
  wait(): Promise<{ server: string; loginName: string; appPassword: string }>
  cancel(): void
}
interface NativeSession {
  readonly userId: string
  start(): Promise<void>
  shutdown(): void
  logout(): Promise<void>
  rooms(): RoomView[]
  refreshRooms(): Promise<void>
  openChat(token: string): Promise<unknown>
  closeChat(token: string): void
  loadOlder(token: string, beforeId: number): Promise<unknown>
  send(token: string, text: string, referenceId: string, replyTo: number | null): Promise<unknown>
  markRead(token: string, messageId: number): Promise<void>
  avatar(token: string, dark: boolean): Promise<{ data: Buffer; contentType: string }>
}
interface NativeCore {
  coreVersion(): string
  startLogin(server: string): Promise<NativeLoginFlow>
  TalkSession: new (server: string, user: string, appPassword: string, onEvent: (e: CoreEvent) => void) => NativeSession
}

const nativePath = app.isPackaged
  ? join(process.resourcesPath, 'native', 'talk_node.node')
  : join(app.getAppPath(), 'native', 'talk_node.node')
// eslint-disable-next-line @typescript-eslint/no-require-imports
const core: NativeCore = require(nativePath)

// --- Persistent state -------------------------------------------------------------

interface StoredAccount {
  server: string
  loginName: string
  /** base64 of safeStorage-encrypted app password */
  secret: string
}

const dataDir = app.getPath('userData')
const accountFile = join(dataDir, 'account.json')
const roomsCacheFile = join(dataDir, 'rooms-cache.json')

function loadAccount(): StoredAccount | null {
  try {
    return JSON.parse(readFileSync(accountFile, 'utf8')) as StoredAccount
  } catch {
    return null
  }
}

function saveAccount(server: string, loginName: string, appPassword: string): void {
  mkdirSync(dataDir, { recursive: true })
  const secret = safeStorage.encryptString(appPassword).toString('base64')
  writeFileSync(accountFile, JSON.stringify({ server, loginName, secret } satisfies StoredAccount), { mode: 0o600 })
}

function clearAccount(): void {
  rmSync(accountFile, { force: true })
  rmSync(roomsCacheFile, { force: true })
}

let roomsCacheTimer: NodeJS.Timeout | undefined
function cacheRooms(rooms: RoomView[]): void {
  clearTimeout(roomsCacheTimer)
  roomsCacheTimer = setTimeout(() => {
    try {
      writeFileSync(roomsCacheFile, JSON.stringify(rooms))
    } catch {
      /* cache is best effort */
    }
  }, 1000)
}

function cachedRooms(): RoomView[] {
  try {
    return JSON.parse(readFileSync(roomsCacheFile, 'utf8')) as RoomView[]
  } catch {
    return []
  }
}

// --- Session -------------------------------------------------------------------------

let win: BrowserWindow | null = null
let session: NativeSession | null = null
let account: AccountInfo | null = null
let loginFlow: NativeLoginFlow | null = null
let activeToken: string | null = null
let lastRooms = new Map<string, RoomView>()
let firstRoomsEvent = true

function send(event: TalkEvent): void {
  win?.webContents.send('talk:event', event)
}

function onCoreEvent(event: CoreEvent): void {
  if (event.type === 'roomsChanged') {
    notifyNewMessages(event.rooms)
    cacheRooms(event.rooms)
  } else if (event.type === 'authFailed') {
    void signOut(false)
    return
  }
  send(event)
}

function startSession(stored: StoredAccount, plainPassword?: string): void {
  const password = plainPassword ?? safeStorage.decryptString(Buffer.from(stored.secret, 'base64'))
  session = new core.TalkSession(stored.server, stored.loginName, password, onCoreEvent)
  firstRoomsEvent = true
  const s = session
  s.start()
    .then(() => {
      if (session !== s) return
      account = { server: stored.server, user: stored.loginName, userId: s.userId }
      send({ type: 'session', state: 'ready', account })
    })
    .catch((err: Error) => {
      // Offline start: keep the session, background sync retries.
      console.warn('session start failed:', err.message)
      if (err.message.startsWith('UNAUTHORIZED')) void signOut(false)
    })
  account = { server: stored.server, user: stored.loginName, userId: stored.loginName }
}

async function signOut(revoke: boolean): Promise<void> {
  const s = session
  session = null
  account = null
  activeToken = null
  lastRooms = new Map()
  if (s) {
    if (revoke) await s.logout().catch(() => s.shutdown())
    else s.shutdown()
  }
  clearAccount()
  app.setBadgeCount(0)
  send({ type: 'session', state: 'loggedOut' } satisfies AppEvent)
}

/**
 * Development only: `TALK_DEV_ENV=../.env TALK_DEV_PREFIX=BOT_A npm run dev` signs in with
 * `<PREFIX>_USER`/`<PREFIX>_PASS` and `NC_URL` from that file, without storing anything.
 */
function devAccount(): { server: string; user: string; password: string } | null {
  const file = process.env.TALK_DEV_ENV
  if (app.isPackaged || !file) return null
  const prefix = process.env.TALK_DEV_PREFIX ?? 'NC'
  const env = Object.fromEntries(
    readFileSync(file, 'utf8')
      .split('\n')
      .filter((l) => l.includes('=') && !l.trimStart().startsWith('#'))
      .map((l) => [l.slice(0, l.indexOf('=')).trim(), l.slice(l.indexOf('=') + 1).trim().replace(/^["']|["']$/g, '')]),
  )
  const user = env[`${prefix}_USER`]
  const password = env[`${prefix}_PASS`]
  return env.NC_URL && user && password ? { server: env.NC_URL, user, password } : null
}

function requireSession(): NativeSession {
  if (!session) throw new Error('not signed in')
  return session
}

// --- Notifications ----------------------------------------------------------------

function wantsNotification(room: RoomView): boolean {
  switch (room.notificationLevel) {
    case 1:
      return true
    case 2:
      return room.unreadMention
    case 3:
      return false
    default:
      return room.roomType === 1 || room.unreadMention
  }
}

function notifyNewMessages(rooms: RoomView[]): void {
  const previous = lastRooms
  lastRooms = new Map(rooms.map((r) => [r.token, r]))
  if (firstRoomsEvent) {
    firstRoomsEvent = false
    return
  }
  const focused = win?.isFocused() ?? false
  for (const room of rooms) {
    const before = previous.get(room.token)
    if (!before || room.unreadMessages <= before.unreadMessages) continue
    if (focused && room.token === activeToken) continue
    if (!wantsNotification(room) || !room.lastMessage || room.lastMessage.isOwn) continue
    if (!Notification.isSupported()) continue
    const text = withAttachment(room.lastMessage.text, room.lastMessage.attachment)
    const n = new Notification({
      title: room.name,
      body: room.roomType === 1 ? text : `${room.lastMessage.author}: ${text}`,
      silent: false,
    })
    n.on('click', () => {
      showWindow()
      send({ type: 'openRoom', token: room.token })
    })
    n.show()
  }
}

// --- Window ------------------------------------------------------------------------

function showWindow(): void {
  if (!win) return
  if (win.isMinimized()) win.restore()
  win.show()
  win.focus()
}

function createWindow(): void {
  win = new BrowserWindow({
    width: 1100,
    height: 760,
    minWidth: 380,
    minHeight: 480,
    show: false,
    title: 'Sirplus Talk',
    autoHideMenuBar: true,
    backgroundColor: nativeTheme.shouldUseDarkColors ? '#121212' : '#ffffff',
    webPreferences: {
      preload: join(__dirname, '../preload/index.js'),
      sandbox: true,
      contextIsolation: true,
      spellcheck: true,
    },
  })
  win.once('ready-to-show', () => win?.show())
  win.on('closed', () => (win = null))

  // Links open in the system browser, never inside the app.
  win.webContents.setWindowOpenHandler(({ url }) => {
    if (/^https?:\/\//.test(url)) void shell.openExternal(url)
    return { action: 'deny' }
  })
  win.webContents.on('will-navigate', (e, url) => {
    if (url !== win?.webContents.getURL()) {
      e.preventDefault()
      if (/^https?:\/\//.test(url)) void shell.openExternal(url)
    }
  })

  if (process.env.ELECTRON_RENDERER_URL) void win.loadURL(process.env.ELECTRON_RENDERER_URL)
  else void win.loadFile(join(__dirname, '../renderer/index.html'))
}

// --- IPC ---------------------------------------------------------------------------

function registerIpc(): void {
  ipcMain.handle('talk:account', () => account)

  ipcMain.handle('talk:login', async (_e, server: string) => {
    loginFlow?.cancel()
    const flow = await core.startLogin(server)
    loginFlow = flow
    await shell.openExternal(flow.loginUrl)
    try {
      const creds = await flow.wait()
      saveAccount(creds.server, creds.loginName, creds.appPassword)
      const stored = loadAccount()
      if (stored) startSession(stored)
      showWindow()
    } finally {
      if (loginFlow === flow) loginFlow = null
    }
  })
  ipcMain.handle('talk:cancelLogin', () => {
    loginFlow?.cancel()
    loginFlow = null
  })
  ipcMain.handle('talk:logout', () => signOut(true))

  ipcMain.handle('talk:rooms', () => {
    if (!session) return []
    const live = session.rooms()
    return live.length > 0 ? live : cachedRooms()
  })
  ipcMain.handle('talk:openChat', (_e, token: string) => {
    activeToken = token
    return requireSession().openChat(token)
  })
  ipcMain.handle('talk:closeChat', (_e, token: string) => {
    if (activeToken === token) activeToken = null
    session?.closeChat(token)
  })
  ipcMain.handle('talk:loadOlder', (_e, token: string, beforeId: number) => requireSession().loadOlder(token, beforeId))
  ipcMain.handle('talk:send', (_e, token: string, text: string, referenceId: string, replyTo: number | null) =>
    requireSession().send(token, text, referenceId, replyTo),
  )
  ipcMain.handle('talk:markRead', (_e, token: string, messageId: number) => session?.markRead(token, messageId))
  ipcMain.on('talk:setBadge', (_e, count: number) => app.setBadgeCount(count))
}

// --- Avatars: talk-avatar://room/<token>?v=<version> ---------------------------------

protocol.registerSchemesAsPrivileged([
  { scheme: 'talk-avatar', privileges: { standard: true, secure: true, supportFetchAPI: true } },
])

const avatarCache = new Map<string, { data: Buffer; contentType: string }>()

function registerAvatarProtocol(): void {
  protocol.handle('talk-avatar', async (request) => {
    const url = new URL(request.url)
    const token = url.pathname.replace(/^\//, '')
    const dark = nativeTheme.shouldUseDarkColors
    const key = `${token}|${url.searchParams.get('v') ?? ''}|${dark}`
    let hit = avatarCache.get(key)
    if (!hit && session) {
      try {
        hit = await session.avatar(token, dark)
        avatarCache.set(key, hit)
      } catch {
        return new Response(null, { status: 404 })
      }
    }
    if (!hit) return new Response(null, { status: 404 })
    return new Response(new Uint8Array(hit.data), {
      headers: { 'content-type': hit.contentType, 'cache-control': 'max-age=86400' },
    })
  })
}

// --- App lifecycle -----------------------------------------------------------------

if (!app.requestSingleInstanceLock()) {
  app.quit()
} else {
  app.on('second-instance', showWindow)
  app.setAppUserModelId('org.sirplus.talk')

  void app.whenReady().then(() => {
    registerIpc()
    registerAvatarProtocol()
    const dev = devAccount()
    const stored = existsSync(accountFile) ? loadAccount() : null
    if (dev) startSession({ server: dev.server, loginName: dev.user, secret: '' }, dev.password)
    else if (stored) startSession(stored)
    createWindow()
    nativeTheme.on('updated', () => avatarCache.clear())
    app.on('activate', () => {
      if (BrowserWindow.getAllWindows().length === 0) createWindow()
    })
  })

  app.on('window-all-closed', () => {
    if (process.platform !== 'darwin') {
      session?.shutdown()
      app.quit()
    }
  })
}
