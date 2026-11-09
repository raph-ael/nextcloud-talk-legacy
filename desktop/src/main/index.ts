import { app, BrowserWindow, ipcMain, nativeTheme, Notification, powerMonitor, protocol, safeStorage, shell } from 'electron'
import { randomBytes } from 'node:crypto'
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { copyFile, readFile, rm, stat, writeFile } from 'node:fs/promises'
import { basename, extname, join, relative, resolve, sep } from 'node:path'
import {
  withAttachment,
  type AccountInfo,
  type AppEvent,
  type CoreEvent,
  type PendingMessage,
  type ReplyPreview,
  type RoomView,
  type StagedFile,
  type TalkEvent,
} from '@shared/core'

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
  cachedChat(token: string): unknown
  openChat(token: string): Promise<unknown>
  closeChat(token: string): void
  loadOlder(token: string, beforeId: number): Promise<unknown>
  markRead(token: string, messageId: number): Promise<void>
  avatar(token: string, dark: boolean): Promise<{ data: Buffer; contentType: string }>
  enqueueText(token: string, text: string, replyTo: ReplyPreview | null, referenceId: string): PendingMessage
  enqueueFile(
    token: string,
    localPath: string,
    name: string,
    mime: string,
    width: number,
    height: number,
    caption: string,
    replyTo: ReplyPreview | null,
    referenceId: string,
  ): PendingMessage
  pending(token: string): PendingMessage[]
  retrySend(referenceId: string): void
  discard(referenceId: string): void
  networkChanged(): void
  outboxUnsent(): number
  waitOutboxIdle(): Promise<{ waiting: number; failed: number }>
  filePreview(fileId: string, width: number, height: number): Promise<Buffer>
}
interface NativeCore {
  coreVersion(): string
  newReferenceId(): string
  startLogin(server: string): Promise<NativeLoginFlow>
  TalkSession: new (
    server: string,
    user: string,
    appPassword: string,
    onEvent: (e: CoreEvent) => void,
    dataDir: string | null,
  ) => NativeSession
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
/** Core cache (conversations, messages, avatars) and outbox of the signed-in account. */
const coreDir = join(dataDir, 'talk')
/** Room list cache of earlier versions (now in the core cache). */
const legacyRoomsCache = join(dataDir, 'rooms-cache.json')

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
  rmSync(legacyRoomsCache, { force: true })
  try {
    // Cached messages belong to this account only.
    rmSync(coreDir, { recursive: true, force: true })
  } catch (err) {
    console.warn('cache cleanup failed:', (err as Error).message)
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
/** Core data folder of the running session (cache, outbox, staging). */
let sessionDir: string | null = null
/** Failed messages already announced by a notification. */
const notifiedFailures = new Set<string>()

function send(event: TalkEvent): void {
  win?.webContents.send('talk:event', event)
}

function onCoreEvent(event: CoreEvent): void {
  if (event.type === 'roomsChanged') {
    notifyNewMessages(event.rooms)
  } else if (event.type === 'authFailed') {
    void signOut(false)
    return
  } else if (event.type === 'outboxChanged') {
    announceFailures(event.token, event.items)
  }
  send(event)
}

function startSession(stored: StoredAccount, plainPassword?: string): void {
  const password = plainPassword ?? safeStorage.decryptString(Buffer.from(stored.secret, 'base64'))
  rmSync(legacyRoomsCache, { force: true })
  // Dev accounts (not stored) get their own cache: never mix two accounts' messages.
  const dir = stored.secret ? coreDir : join(dataDir, 'talk-dev', stored.loginName)
  session = new core.TalkSession(stored.server, stored.loginName, password, onCoreEvent, dir)
  sessionDir = dir
  // Leftovers of attachments that were picked but never sent.
  rmSync(join(dir, 'staging'), { recursive: true, force: true })
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
  sessionDir = null
  notifiedFailures.clear()
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

/** A message finally failed while the user was not looking at its chat. */
function announceFailures(token: string, items: PendingMessage[]): void {
  const fresh = items.filter((i) => i.state === 'failed' && !notifiedFailures.has(i.message.referenceId))
  if (fresh.length === 0) return
  fresh.forEach((i) => notifiedFailures.add(i.message.referenceId))
  const visible = (win?.isFocused() ?? false) && activeToken === token
  if (visible || !Notification.isSupported()) return
  const n = new Notification({
    title: 'Nachricht nicht gesendet',
    body: lastRooms.get(token)?.name ?? 'Unterhaltung',
  })
  n.on('click', () => {
    showWindow()
    send({ type: 'openRoom', token })
  })
  n.show()
}

// --- Attachments: staging ----------------------------------------------------------

function stagingDir(): string {
  if (!sessionDir) throw new Error('not signed in')
  const dir = join(sessionDir, 'staging')
  mkdirSync(dir, { recursive: true })
  return dir
}

/** File name without path parts or characters that confuse file systems. */
function safeName(name: string): string {
  const clean = basename(name).replace(/[\\/:*?"<>|\x00-\x1f]/g, '_').trim()
  return clean || 'Datei'
}

function stagedPath(name: string): string {
  return join(stagingDir(), `${randomBytes(8).toString('hex')}${extname(safeName(name)).toLowerCase()}`)
}

/** `true` when `path` lies inside `dir` (no `..` escapes). */
function inside(dir: string, path: string): boolean {
  const rel = relative(resolve(dir), resolve(path))
  return rel !== '' && !rel.startsWith('..') && !rel.startsWith(sep) && !/^[a-zA-Z]:/.test(rel)
}

const MIME_BY_EXT: Record<string, string> = {
  '.jpg': 'image/jpeg',
  '.jpeg': 'image/jpeg',
  '.png': 'image/png',
  '.gif': 'image/gif',
  '.webp': 'image/webp',
  '.svg': 'image/svg+xml',
  '.pdf': 'application/pdf',
  '.mp4': 'video/mp4',
  '.webm': 'video/webm',
  '.mp3': 'audio/mpeg',
  '.ogg': 'audio/ogg',
  '.txt': 'text/plain',
  '.zip': 'application/zip',
}

function mimeFor(name: string, given: string): string {
  return given || MIME_BY_EXT[extname(name).toLowerCase()] || 'application/octet-stream'
}

async function stageData(name: string, mime: string, data: Uint8Array, width: number, height: number): Promise<StagedFile> {
  const path = stagedPath(name)
  await writeFile(path, data)
  return { path, name: safeName(name), mime: mimeFor(name, mime), size: data.byteLength, width, height }
}

async function stagePath(source: string, name: string, mime: string): Promise<StagedFile> {
  const info = await stat(source)
  if (!info.isFile()) throw new Error('Ordner können nicht gesendet werden')
  const path = stagedPath(name || source)
  // A copy: the outbox moves its file, the user's original must stay.
  await copyFile(source, path)
  const fileName = safeName(name || basename(source))
  return { path, name: fileName, mime: mimeFor(fileName, mime), size: info.size, width: 0, height: 0 }
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

  // From the core cache before the first request completes.
  ipcMain.handle('talk:rooms', () => session?.rooms() ?? [])
  ipcMain.handle('talk:cachedChat', (_e, token: string) => requireSession().cachedChat(token))
  ipcMain.handle('talk:openChat', (_e, token: string) => {
    activeToken = token
    return requireSession().openChat(token)
  })
  ipcMain.handle('talk:closeChat', (_e, token: string) => {
    if (activeToken === token) activeToken = null
    session?.closeChat(token)
  })
  ipcMain.handle('talk:loadOlder', (_e, token: string, beforeId: number) => requireSession().loadOlder(token, beforeId))
  ipcMain.handle('talk:refreshRooms', () => session?.refreshRooms().catch(() => undefined))
  ipcMain.handle('talk:enqueueText', (_e, token: string, text: string, replyTo: ReplyPreview | null) =>
    requireSession().enqueueText(token, text, replyTo, core.newReferenceId()),
  )
  ipcMain.handle(
    'talk:enqueueFile',
    (_e, token: string, file: StagedFile, caption: string, replyTo: ReplyPreview | null) => {
      if (!sessionDir || !inside(join(sessionDir, 'staging'), file.path)) throw new Error('not a staged file')
      return requireSession().enqueueFile(
        token,
        file.path,
        file.name,
        file.mime,
        file.width,
        file.height,
        caption,
        replyTo,
        core.newReferenceId(),
      )
    },
  )
  ipcMain.handle('talk:pending', (_e, token: string) => session?.pending(token) ?? [])
  ipcMain.handle('talk:retrySend', (_e, referenceId: string) => {
    notifiedFailures.delete(referenceId)
    session?.retrySend(referenceId)
  })
  ipcMain.handle('talk:discard', (_e, referenceId: string) => session?.discard(referenceId))
  ipcMain.on('talk:networkChanged', () => session?.networkChanged())
  ipcMain.handle('talk:stageData', (_e, name: string, mime: string, data: Uint8Array, width: number, height: number) =>
    stageData(name, mime, data, width, height),
  )
  ipcMain.handle('talk:stagePath', (_e, path: string, name: string, mime: string) => stagePath(path, name, mime))
  ipcMain.handle('talk:unstage', async (_e, path: string) => {
    if (sessionDir && inside(join(sessionDir, 'staging'), path)) await rm(path, { force: true })
  })
  ipcMain.handle('talk:markRead', (_e, token: string, messageId: number) => session?.markRead(token, messageId))
  ipcMain.on('talk:setBadge', (_e, count: number) => app.setBadgeCount(count))
}

// --- Avatars: talk-avatar://room/<token>?v=<version> ---------------------------------

protocol.registerSchemesAsPrivileged([
  { scheme: 'talk-avatar', privileges: { standard: true, secure: true, supportFetchAPI: true } },
  { scheme: 'talk-media', privileges: { standard: true, secure: true, supportFetchAPI: true } },
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

// --- Media: talk-media://preview/<fileId>?w=&h=, talk-media://local/file?p=<path> ---

function imageType(data: Uint8Array, fallback: string): string {
  if (data[0] === 0xff && data[1] === 0xd8) return 'image/jpeg'
  if (data[0] === 0x89 && data[1] === 0x50) return 'image/png'
  if (data[0] === 0x47 && data[1] === 0x49) return 'image/gif'
  if (data[8] === 0x57 && data[9] === 0x45) return 'image/webp'
  return fallback
}

function registerMediaProtocol(): void {
  protocol.handle('talk-media', async (request) => {
    const url = new URL(request.url)
    try {
      if (url.hostname === 'preview' && session) {
        const fileId = decodeURIComponent(url.pathname.replace(/^\//, ''))
        const w = Math.min(Number(url.searchParams.get('w')) || 512, 4096)
        const h = Math.min(Number(url.searchParams.get('h')) || 512, 4096)
        const data = await session.filePreview(fileId, w, h)
        return new Response(new Uint8Array(data), {
          headers: { 'content-type': imageType(data, 'image/jpeg'), 'cache-control': 'max-age=604800' },
        })
      }
      if (url.hostname === 'local' && sessionDir) {
        // Only files of the outbox/staging of this account, nothing else on disk.
        const path = url.searchParams.get('p') ?? ''
        if (!inside(sessionDir, path)) return new Response(null, { status: 403 })
        const data = await readFile(path)
        return new Response(new Uint8Array(data), {
          headers: { 'content-type': imageType(data, mimeFor(path, '')) },
        })
      }
    } catch {
      return new Response(null, { status: 404 })
    }
    return new Response(null, { status: 404 })
  })
}

/** Gives the outbox a moment to deliver what is in flight before the process ends. */
async function quitAfterOutbox(): Promise<void> {
  const s = session
  if (s && s.outboxUnsent() > 0) {
    await Promise.race([s.waitOutboxIdle(), new Promise((r) => setTimeout(r, 10_000))]).catch(() => undefined)
  }
  s?.shutdown()
  app.quit()
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
    registerMediaProtocol()
    powerMonitor.on('resume', () => session?.networkChanged())
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
    if (process.platform !== 'darwin') void quitAfterOutbox()
  })
}
