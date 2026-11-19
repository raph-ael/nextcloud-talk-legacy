import {
  app,
  BrowserWindow,
  ipcMain,
  Menu,
  nativeImage,
  nativeTheme,
  Notification,
  protocol,
  safeStorage,
  session as electronSession,
  shell,
  Tray,
} from 'electron'
import { randomBytes } from 'node:crypto'
import { createReadStream, existsSync, mkdirSync, readdirSync, readFileSync, renameSync, rmSync, writeFileSync } from 'node:fs'
import { copyFile, open, readFile, rm, stat, writeFile } from 'node:fs/promises'
import { Readable } from 'node:stream'
import { basename, extname, join, relative, resolve, sep } from 'node:path'
import { callChanged, ensureCamera, ensureMicrophone, installSystem, recordingsChanged, unreadChanged } from './system'
import { autostartAvailable, autostartOn, setAutostart, startedAtLogin } from './autostart'
import { installScreenShare } from './screen'
import { checkForUpdatesNow, installAutoUpdate, updatesActive } from './update'
import {
  recordingNoticeTitle,
  withAttachment,
  type AccountInfo,
  type AccountsView,
  type AppEvent,
  type AudioInfo,
  type CallView,
  type ConnectionState,
  type MediaCommand,
  type MediaReport,
  type MessageNotice,
  type CoreEvent,
  type PendingMessage,
  type RecordingNotice,
  type MentionPick,
  type ReplyPreview,
  type RoomView,
  type StagedFile,
  type TalkEvent,
  type VoiceDraft,
  type RecordingStatus,
  type Theme,
  type Appearance,
  type Layout,
} from '@shared/core'
import { ACCENTS, DEFAULT_ACCENT } from '../renderer/src/accents'

/** A picture from `resources/` (design/icon/build-icons.py); the @2x file next to it is picked up for HiDPI. */
function resource(name: string): string {
  return join(__dirname, '../../resources', name)
}

// --- Native core -------------------------------------------------------------------

interface NativeLoginFlow {
  readonly loginUrl: string
  wait(): Promise<{ server: string; loginName: string; appPassword: string }>
  cancel(): void
}
interface NativeSession {
  readonly userId: string
  /** From the cache at once, up to date after `start()` (docs/accounts.md). */
  readonly displayName: string
  badge(): Badge
  start(): Promise<void>
  shutdown(): void
  logout(): Promise<void>
  rooms(): RoomView[]
  refreshRooms(): Promise<void>
  cachedChat(token: string): unknown
  openChat(token: string): Promise<unknown>
  closeChat(token: string): void
  loadOlder(token: string, beforeId: number): Promise<unknown>
  messagesAround(token: string, messageId: number): Promise<unknown>
  loadNewer(token: string, afterId: number): Promise<unknown>
  searchMessages(query: string, token: string | null, limit: number): Promise<unknown>
  searchMessagesEverywhere(query: string, token: string | null, limit: number): Promise<unknown>
  markRead(token: string, messageId: number): Promise<void>
  setReaction(token: string, messageId: number, emoji: string | null): Promise<string | null>
  editSource(token: string, messageId: number): Promise<unknown>
  editMessage(token: string, messageId: number, text: string, mentions: MentionPick[]): Promise<string | null>
  mentionCandidates(token: string, query: string, limit: number): Promise<unknown>
  existingDirect(userId: string): string | null
  deleteMessage(token: string, messageId: number): Promise<string | null>
  reactions(token: string, messageId: number): Promise<unknown>
  roomSections(query: string, activeToken: string | null, split?: boolean): unknown
  toggleSection(id: string): Promise<string | null>
  sortOrder(): string
  setSortOrder(order: string): Promise<string | null>
  categoriesSupported(): boolean
  categories(): unknown
  createCategory(name: string, tokens: string[]): Promise<unknown>
  renameCategory(id: string, name: string): Promise<string | null>
  deleteCategory(id: string): Promise<string | null>
  moveCategory(id: string, offset: number): Promise<string | null>
  setCategoryRooms(id: string, tokens: string[]): Promise<string | null>
  canCreateGroups(): boolean
  searchUsers(query: string, excludeToken: string | null): Promise<unknown>
  openDirect(userId: string): Promise<unknown>
  createGroup(name: string, userIds: string[], photo: Buffer | null): Promise<unknown>
  members(token: string): Promise<unknown>
  addMembers(token: string, userIds: string[]): Promise<string | null>
  removeMember(token: string, attendeeId: number): Promise<string | null>
  setModerator(token: string, attendeeId: number, moderator: boolean): Promise<string | null>
  renameGroup(token: string, name: string): Promise<string | null>
  setDescription(token: string, description: string): Promise<string | null>
  setGroupPhoto(token: string, photo: Buffer | null): Promise<string | null>
  setGroupLink(token: string, on: boolean): Promise<string | null>
  groupLink(token: string): string
  setNotificationLevel(token: string, level: number): Promise<string | null>
  setFavorite(token: string, on: boolean): Promise<string | null>
  setRoomCategories(token: string, ids: string[]): Promise<string | null>
  markRoomRead(token: string): Promise<string | null>
  leaveNeedsModerator(token: string): Promise<boolean>
  leaveGroup(token: string): Promise<string | null>
  deleteGroup(token: string): Promise<string | null>
  avatar(token: string, dark: boolean): Promise<{ data: Buffer; contentType: string }>
  enqueueText(token: string, text: string, mentions: MentionPick[], replyTo: ReplyPreview | null, referenceId: string): PendingMessage
  enqueueFile(
    token: string,
    localPath: string,
    name: string,
    mime: string,
    width: number,
    height: number,
    caption: string,
    mentions: MentionPick[],
    replyTo: ReplyPreview | null,
    referenceId: string,
  ): PendingMessage
  pending(token: string): PendingMessage[]
  retrySend(referenceId: string): void
  discard(referenceId: string): void
  networkChanged(): void
  networkLost(): void
  connectionState(): ConnectionState
  outboxUnsent(): number
  waitOutboxIdle(): Promise<{ waiting: number; failed: number }>
  filePreview(fileId: string, width: number, height: number): Promise<Buffer>
  startVoice(token: string, sampleRate: number): NativeVoiceRecording
  voiceDraft(token: string): VoiceDraft | null
  discardVoiceDraft(token: string): void
  sendVoiceDraft(token: string, name: string, replyTo: ReplyPreview | null, referenceId: string): PendingMessage
  fetchFile(path: string, size: number): Promise<string>
  audioInfo(localPath: string): Promise<AudioInfo>
  setMediaEngine(onCommand: (command: MediaCommand) => void): void
  currentCall(): CallView | null
  startCall(token: string, silent: boolean, callId?: string | null): Promise<void>
  hangUpAndWait(timeoutMs: number): Promise<void>
  acceptCall(token: string): Promise<void>
  declineCall(token: string): void
  hangUp(): void
  setCallMuted(muted: boolean): void
  setCameraEnabled(on: boolean): void
  setScreenShare(on: boolean): void
  startRecording(video: boolean): Promise<void>
  stopRecording(): Promise<void>
  consentToRecording(): void
  recordingNotices(): RecordingNotice[]
  shareRecording(id: number): Promise<void>
  dismissRecording(id: number): Promise<void>
  userAvatar(userId: string, dark: boolean): Promise<{ data: Buffer; contentType: string }>
  mediaReport(report: MediaReport): void
}
interface Badge {
  chats: number
  messages: number
}
interface NativeVoiceRecording {
  push(pcm: Buffer): RecordingStatus
  finish(): VoiceDraft | null
  cancel(): void
}
interface NativeCore {
  coreVersion(): string
  newReferenceId(): string
  startLogin(server: string): Promise<NativeLoginFlow>
  resolveAccount(server: string, loginName: string, appPassword: string): Promise<{ userId: string; displayName: string; key: string }>
  revokeAppPassword(server: string, loginName: string, appPassword: string): Promise<void>
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

const dataDir = app.getPath('userData')
migrateLegacyDataDir()
const settingsFile = join(dataDir, 'settings.json')

interface Settings {
  /** Closing the window keeps the app in the tray, so calls can ring (Linux/Windows). */
  closeToTray: boolean
  /** „Darstellung“ (docs/settings.md): device only, kept when signing out. */
  theme: Theme
  /** Accent id from design/theme/accents.json; unknown ids show as the default. */
  accent: string
  /** „Beim Anmelden starten“ (docs/notifications.md). */
  openAtLogin: boolean
  /** „Ansicht“: bubbles or the compact view (docs/compact-view.md). */
  layout: Layout
  /** „Gruppen und Direktchats trennen“ (docs/categories.md). */
  splitChats: boolean
}

const THEMES: readonly Theme[] = ['system', 'light', 'dark']
const LAYOUTS: readonly Layout[] = ['bubbles', 'compact']

function loadSettings(): Settings {
  const defaults: Settings = {
    closeToTray: true,
    theme: 'system',
    accent: DEFAULT_ACCENT,
    openAtLogin: false,
    layout: 'bubbles',
    splitChats: true,
  }
  try {
    const stored = { ...defaults, ...(JSON.parse(readFileSync(settingsFile, 'utf8')) as Partial<Settings>) }
    if (!THEMES.includes(stored.theme)) stored.theme = 'system'
    if (!ACCENTS.some((a) => a.id === stored.accent)) stored.accent = DEFAULT_ACCENT
    stored.openAtLogin = stored.openAtLogin === true
    if (!LAYOUTS.includes(stored.layout)) stored.layout = 'bubbles'
    stored.splitChats = stored.splitChats !== false
    return stored
  } catch {
    return defaults
  }
}

const settings = loadSettings()

function saveSettings(): void {
  try {
    mkdirSync(dataDir, { recursive: true })
    writeFileSync(settingsFile, JSON.stringify(settings))
  } catch (err) {
    console.warn('saving settings failed:', (err as Error).message)
  }
}

/**
 * The app was called "Sirplus Talk" before it became Wolkenfunk: carry its data folder over
 * (account, cache, settings), once, before Chromium uses the new one. Only into the default
 * folder: a run with its own --user-data-dir (tests, a second profile) must not take it.
 */
function migrateLegacyDataDir(): void {
  if (app.commandLine.hasSwitch('user-data-dir')) return
  const legacy = join(app.getPath('appData'), 'Sirplus Talk')
  if (!existsSync(legacy) || existsSync(join(dataDir, 'account.json')) || existsSync(join(dataDir, 'accounts.json'))) return
  try {
    mkdirSync(dataDir, { recursive: true })
    for (const entry of readdirSync(legacy)) {
      if (!existsSync(join(dataDir, entry))) renameSync(join(legacy, entry), join(dataDir, entry))
    }
    rmSync(legacy, { recursive: true, force: true })
  } catch (err) {
    console.warn('moving the old data folder failed:', (err as Error).message)
  }
}

// --- Account list (docs/accounts.md „Speicherung“) -----------------------------------

/** One entry of `accounts.json`. */
interface StoredAccount {
  /** Random local id (hex), never reused: folder names, URLs, IPC. */
  id: string
  server: string
  loginName: string
  userId: string
  /** Display name from the server, refreshed after every start. */
  name: string
  /** From `resolveAccount`: the same user on the same server (empty until known). */
  key: string
  /** Data folder relative to userData: `talk` (the account from before) or `accounts/<id>`. */
  dir: string
  /** base64 of the safeStorage-encrypted app password. */
  secret: string
}

interface AccountsFile {
  /** In the order of signing in. */
  accounts: StoredAccount[]
  /** The account shown after a restart. */
  active: string | null
}

const accountsFile = join(dataDir, 'accounts.json')
/** The single account of earlier versions; taken over once (`adoptLegacyAccount`). */
const legacyAccountFile = join(dataDir, 'account.json')
/** Room list cache of earlier versions (now in the core cache). */
const legacyRoomsCache = join(dataDir, 'rooms-cache.json')
const MAX_ACCOUNTS = 5

let list: AccountsFile = { accounts: [], active: null }
/** Dev runs (`TALK_DEV_*`) keep their accounts out of the list. */
let persist = true

function readAccountsFile(): AccountsFile {
  try {
    const f = JSON.parse(readFileSync(accountsFile, 'utf8')) as Partial<AccountsFile>
    return { accounts: Array.isArray(f.accounts) ? f.accounts : [], active: f.active ?? null }
  } catch (err) {
    if ((err as NodeJS.ErrnoException).code !== 'ENOENT') console.warn('reading accounts.json failed:', (err as Error).message)
    return { accounts: [], active: null }
  }
}

/** Atomic: a crash while writing leaves the previous list, never half a file. */
function saveAccounts(): boolean {
  if (!persist) return true
  try {
    mkdirSync(dataDir, { recursive: true })
    const tmp = `${accountsFile}.tmp`
    writeFileSync(tmp, JSON.stringify(list), { mode: 0o600 })
    renameSync(tmp, accountsFile)
    return true
  } catch (err) {
    console.warn('saving the account list failed:', (err as Error).message)
    return false
  }
}

function newAccountId(): string {
  for (;;) {
    const id = randomBytes(6).toString('hex')
    if (!list.accounts.some((a) => a.id === id) && !existsSync(join(dataDir, 'accounts', id))) return id
  }
}

function encrypt(appPassword: string): string {
  return safeStorage.encryptString(appPassword).toString('base64')
}

/** Throws when the keyring does not open it (locked, refused): not „no account“. */
function decrypt(secret: string): string {
  return safeStorage.decryptString(Buffer.from(secret, 'base64'))
}

// --- Runtimes: one started session per account -------------------------------------------

interface Runtime {
  id: string
  stored: StoredAccount
  session: NativeSession
  /** Absolute data folder (cache, outbox, staging). */
  dir: string
  /** The chat open in the window (only the active account has one). */
  openToken: string | null
  /** Failed messages already announced by a notification. */
  notifiedFailures: Set<string>
  /** Recording notices already announced. */
  knownNotices: Set<number>
  /** Unix s of the start: older notices (stored while the app was closed) stay quiet. */
  noticesSince: number
  badge: Badge
}

/** In the order of the list; accounts whose secret did not open in this run are missing. */
const runtimes = new Map<string, Runtime>()
/** The account the window shows. */
let activeId: string | null = null

let win: BrowserWindow | null = null
let loginFlow: NativeLoginFlow | null = null
/** The voice recording in progress (encoded and written by the core in this process). */
let recording: { rt: Runtime; token: string; rec: NativeVoiceRecording } | null = null

function activeRuntime(): Runtime | null {
  return activeId ? (runtimes.get(activeId) ?? null) : null
}

/** Events of the app itself (no account). */
function send(event: TalkEvent): void {
  win?.webContents.send('talk:event', event)
}

/** Events of an account reach the window only while it is the active one. */
function sendFrom(rt: Runtime, event: TalkEvent): void {
  if (rt.id === activeId) send({ ...event, accountId: rt.id })
}

function serverLabel(server: string): string {
  return server.replace(/^https?:\/\//, '').replace(/\/+$/, '')
}

function accountInfo(rt: Runtime): AccountInfo {
  const s = rt.stored
  return { id: rt.id, server: s.server, user: s.loginName, userId: s.userId || s.loginName, name: s.name || s.userId || s.loginName }
}

/**
 * What the window needs to show an account. The connection state is kept per account by
 * its session, also while its events do not reach the window (docs/connection.md).
 */
function shownAccount(rt: Runtime): { account: AccountInfo; connection: ConnectionState } {
  return { account: accountInfo(rt), connection: rt.session.connectionState() }
}

/** „{Name} · {Server}“ for notifications, only with two accounts or more. */
function accountLine(rt: Runtime): string | null {
  if (runtimes.size < 2) return null
  return `${accountInfo(rt).name} · ${serverLabel(rt.stored.server)}`
}

function accountsView(): AccountsView {
  return {
    active: activeId,
    accounts: [...runtimes.values()].map((rt) => {
      const a = accountInfo(rt)
      return { id: a.id, name: a.name, server: serverLabel(a.server), userId: a.userId, chats: rt.badge.chats }
    }),
  }
}

function sendAccounts(): void {
  send({ type: 'accountsChanged', ...accountsView() } satisfies AppEvent)
}

/** App badge: the sum over all accounts (docs/accounts.md); per account in the switcher. */
function updateBadges(): void {
  let changed = false
  let total = 0
  for (const rt of runtimes.values()) {
    let b: Badge
    try {
      b = rt.session.badge()
    } catch {
      b = rt.badge
    }
    if (b.chats !== rt.badge.chats || b.messages !== rt.badge.messages) changed = true
    rt.badge = b
    total += b.chats
  }
  app.setBadgeCount(total)
  showUnread(total)
  if (changed) sendAccounts()
}

function onCoreEvent(rt: Runtime, event: CoreEvent): void {
  if (!runtimes.has(rt.id)) return
  switch (event.type) {
    case 'callChanged':
      // The one call of the process, whichever account it belongs to.
      onCallChanged(rt, event.call)
      return
    case 'callMissed':
      notifyMissedCall(rt, event)
      break
    case 'roomsChanged':
      updateBadges()
      break
    case 'messageNotices':
      notifyMessages(rt, event.items, event.sinceStart)
      return
    case 'noticesCleared':
      closeNotices(rt, event.tokens)
      return
    case 'authFailed':
      void removeAccount(rt, false)
      return
    case 'outboxChanged':
      announceFailures(rt, event.token, event.items)
      break
    case 'recordingsChanged':
      notifyRecordings(rt, event.items)
      recordingsChanged(rt.id, event.items)
      break
  }
  sendFrom(rt, event)
}

/** Builds the session of an account (no network yet: `bootRuntime` starts it). */
function createRuntime(stored: StoredAccount, password: string): Runtime {
  const dir = join(dataDir, stored.dir)
  // Leftovers of attachments that were picked but never sent.
  rmSync(join(dir, 'staging'), { recursive: true, force: true })
  const rt = {
    id: stored.id,
    stored,
    dir,
    openToken: null,
    notifiedFailures: new Set(),
    knownNotices: new Set(),
    noticesSince: Math.floor(Date.now() / 1000),
    badge: { chats: 0, messages: 0 },
  } as Omit<Runtime, 'session'> as Runtime
  rt.session = new core.TalkSession(stored.server, stored.loginName, password, (e) => onCoreEvent(rt, e), dir)
  rt.session.setMediaEngine((cmd) => onMediaCommand(rt, cmd))
  runtimes.set(rt.id, rt)
  return rt
}

/** `start()`; afterwards the name, the user id and (once) the key are refreshed. */
function bootRuntime(rt: Runtime, password: string): void {
  const s = rt.session
  s.start()
    .then(async () => {
      if (runtimes.get(rt.id) !== rt) return
      const changed = rt.stored.name !== s.displayName || rt.stored.userId !== s.userId
      rt.stored.name = s.displayName
      rt.stored.userId = s.userId
      if (!rt.stored.key) {
        const who = await core.resolveAccount(rt.stored.server, rt.stored.loginName, password).catch(() => null)
        if (who && runtimes.get(rt.id) === rt) rt.stored.key = who.key
      }
      saveAccounts()
      if (changed) sendAccounts()
      if (rt.id === activeId) sendFrom(rt, { type: 'session', state: 'ready', ...shownAccount(rt) })
    })
    .catch((err: Error) => {
      // Offline start: keep the session, background sync retries.
      console.warn('session start failed:', err.message)
      if (err.message.startsWith('UNAUTHORIZED')) void removeAccount(rt, false)
    })
}

/**
 * All stored accounts, plus the one of earlier versions (once). An account whose secret
 * does not open stays in the list and is tried again at the next start.
 */
function startStoredAccounts(): void {
  rmSync(legacyRoomsCache, { force: true })
  list = readAccountsFile()
  for (const stored of list.accounts) {
    let password: string
    try {
      password = decrypt(stored.secret)
    } catch (err) {
      console.warn('account not opened in this run:', (err as Error).message)
      continue
    }
    bootRuntime(createRuntime(stored, password), password)
  }
  adoptLegacyAccount()
  activeId = list.active && runtimes.has(list.active) ? list.active : ([...runtimes.keys()][0] ?? null)
  updateBadges()
}

/**
 * The account of version 0.3 and earlier becomes the first entry, without signing in again.
 * It keeps its folder `talk/` (the outbox remembers absolute paths of waiting attachments)
 * and its secret as it is; `account.json` goes only once the new list is written.
 */
function adoptLegacyAccount(): void {
  if (!existsSync(legacyAccountFile)) return
  if (list.accounts.some((a) => a.dir === 'talk')) {
    rmSync(legacyAccountFile, { force: true })
    return
  }
  let old: { server: string; loginName: string; secret: string }
  let password: string
  try {
    old = JSON.parse(readFileSync(legacyAccountFile, 'utf8')) as typeof old
    password = decrypt(old.secret)
  } catch (err) {
    // Locked or refused keyring: nothing is deleted, the next start tries again.
    console.warn('earlier account not taken over yet:', (err as Error).message)
    return
  }
  const stored: StoredAccount = {
    id: newAccountId(),
    server: old.server,
    loginName: old.loginName,
    userId: '',
    name: '',
    key: '',
    dir: 'talk',
    secret: old.secret,
  }
  const rt = createRuntime(stored, password)
  // From the cache, without network; the key follows after the first start.
  stored.userId = rt.session.userId
  stored.name = rt.session.displayName
  list.accounts.unshift(stored)
  list.active ??= stored.id
  if (saveAccounts()) rmSync(legacyAccountFile, { force: true })
  bootRuntime(rt, password)
  // The runtime map follows the list: the adopted account comes first.
  const others = [...runtimes.values()].filter((r) => r !== rt)
  runtimes.clear()
  for (const r of [rt, ...others]) runtimes.set(r.id, r)
}

/**
 * Shows another account (docs/accounts.md „Wechseln“): the open chat closes, the window
 * gets that account's list at once. Refused during a call unless the call itself switches.
 */
function switchTo(id: string, force = false): string | null {
  const next = runtimes.get(id)
  if (!next) return 'Dieses Konto ist nicht mehr angemeldet'
  if (id === activeId) return null
  if (!force && callRunning()) return 'Während eines Anrufs nicht möglich'
  const prev = activeRuntime()
  if (prev?.openToken) {
    prev.session.closeChat(prev.openToken)
    prev.openToken = null
  }
  activeId = id
  list.active = id
  saveAccounts()
  sendFrom(next, { type: 'session', state: 'ready', ...shownAccount(next), rooms: next.session.rooms() })
  sendAccounts()
  return null
}

/** Only this account: its call, recording, entry, secret and data folder (docs/accounts.md „Abmelden“). */
async function removeAccount(rt: Runtime, revoke: boolean): Promise<void> {
  if (runtimes.get(rt.id) !== rt) return
  runtimes.delete(rt.id)
  if (recording?.rt === rt) {
    recording.rec.cancel()
    recording = null
  }
  if (callOwner === rt) {
    // Leave a running call before the app password goes away.
    if (callActive()) await rt.session.hangUpAndWait(3000).catch(() => undefined)
    callEnded()
  }
  if (mediaOwner === rt) mediaOwner = null
  list.accounts = list.accounts.filter((a) => a.id !== rt.id)
  if (list.active === rt.id) list.active = [...runtimes.keys()][0] ?? null
  saveAccounts()
  if (activeId === rt.id) {
    activeId = null
    const next = [...runtimes.values()][0]
    if (next) switchTo(next.id, true)
    else send({ type: 'session', state: 'loggedOut' } satisfies AppEvent)
  }
  for (const key of avatarCache.keys()) if (key.startsWith(`${rt.id}|`)) avatarCache.delete(key)
  closeNotices(rt, null)
  updateBadges()
  sendAccounts()
  if (!revoke) announceSignedOut(rt)
  if (revoke) await rt.session.logout().catch(() => rt.session.shutdown())
  else rt.session.shutdown()
  try {
    // Cached messages and the outbox belong to this account only.
    rmSync(rt.dir, { recursive: true, force: true })
  } catch (err) {
    console.warn('cache cleanup failed:', (err as Error).message)
  }
}

/** „Anmeldung abgelaufen“: toast in the window, a notification when it is not in front. */
function announceSignedOut(rt: Runtime): void {
  const a = accountInfo(rt)
  const text = `${a.name} (${serverLabel(a.server)}) wurde abgemeldet. Bitte neu anmelden.`
  send({ type: 'notice', text } satisfies AppEvent)
  if (win?.isFocused() || !Notification.isSupported()) return
  const n = hold(new Notification({ title: 'Wolkenfunk', body: text }))
  n.on('click', showWindow)
  n.show()
}

/**
 * „Konto hinzufügen“ and the first sign-in: browser login, then `resolveAccount`; only then
 * is anything stored. An account that is signed in already gets the window instead.
 */
async function addAccount(server: string): Promise<'added' | 'duplicate'> {
  loginFlow?.cancel()
  const flow = await core.startLogin(server)
  loginFlow = flow
  await shell.openExternal(flow.loginUrl)
  try {
    const creds = await flow.wait()
    const who = await core.resolveAccount(creds.server, creds.loginName, creds.appPassword)
    showWindow()
    const same = [...runtimes.values()].find((r) => r.stored.key === who.key)
    if (same) {
      void core.revokeAppPassword(creds.server, creds.loginName, creds.appPassword).catch(() => undefined)
      switchTo(same.id)
      return 'duplicate'
    }
    // An entry whose secret did not open in this run: the fresh password repairs it, with
    // its folder and outbox.
    const stale = list.accounts.find((a) => a.key === who.key && !runtimes.has(a.id))
    if (stale) {
      stale.secret = persist ? encrypt(creds.appPassword) : ''
      stale.loginName = creds.loginName
      saveAccounts()
      bootRuntime(createRuntime(stale, creds.appPassword), creds.appPassword)
      switchTo(stale.id, true)
      updateBadges()
      sendAccounts()
      return 'added'
    }
    if (runtimes.size >= MAX_ACCOUNTS) throw new Error('TOO_MANY_ACCOUNTS')
    const id = newAccountId()
    const stored: StoredAccount = {
      id,
      server: creds.server,
      loginName: creds.loginName,
      userId: who.userId,
      name: who.displayName,
      key: who.key,
      dir: persist ? join('accounts', id) : join('talk-dev', id),
      secret: persist ? encrypt(creds.appPassword) : '',
    }
    list.accounts.push(stored)
    saveAccounts()
    bootRuntime(createRuntime(stored, creds.appPassword), creds.appPassword)
    switchTo(id, true)
    updateBadges()
    sendAccounts()
    return 'added'
  } finally {
    if (loginFlow === flow) loginFlow = null
  }
}

/** Ends a recording whose renderer went away: it stays as the chat's draft. */
function finishRecording(): VoiceDraft | null {
  const r = recording
  recording = null
  if (!r) return null
  try {
    return r.rec.finish()
  } catch (err) {
    console.warn('finishing recording failed:', (err as Error).message)
    return null
  }
}

/** "Sprachnachricht 2026-11-09 15-04-33" in local time (Talk adds " (2)" on clashes). */
function voiceFileName(): string {
  const d = new Date()
  const p = (n: number) => String(n).padStart(2, '0')
  return `Sprachnachricht ${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}-${p(d.getMinutes())}-${p(d.getSeconds())}`
}

/**
 * Development only, never stored and never mixed with the stored accounts:
 * `TALK_DEV_SERVER`/`_USER`/`_PASS` (and the same with `_2` for a second account), or
 * `TALK_DEV_ENV=../.env TALK_DEV_PREFIX=BOT_A` with `<PREFIX>_USER`/`_PASS` and `NC_URL`.
 */
function devAccounts(): { server: string; user: string; password: string }[] {
  if (app.isPackaged) return []
  const found: { server: string; user: string; password: string }[] = []
  for (const suffix of ['', '_2']) {
    const server = process.env[`TALK_DEV_SERVER${suffix}`]
    const user = process.env[`TALK_DEV_USER${suffix}`]
    const password = process.env[`TALK_DEV_PASS${suffix}`]
    if (server && user && password) found.push({ server, user, password })
  }
  if (found.length) return found
  const file = process.env.TALK_DEV_ENV
  if (!file) return []
  const prefix = process.env.TALK_DEV_PREFIX ?? 'NC'
  const env = Object.fromEntries(
    readFileSync(file, 'utf8')
      .split('\n')
      .filter((l) => l.includes('=') && !l.trimStart().startsWith('#'))
      .map((l) => [l.slice(0, l.indexOf('=')).trim(), l.slice(l.indexOf('=') + 1).trim().replace(/^["']|["']$/g, '')]),
  )
  const user = env[`${prefix}_USER`]
  const password = env[`${prefix}_PASS`]
  return env.NC_URL && user && password ? [{ server: env.NC_URL, user, password }] : []
}

function startDevAccounts(dev: { server: string; user: string; password: string }[]): void {
  persist = false
  for (const d of dev) {
    const host = serverLabel(d.server).replace(/[^a-zA-Z0-9.-]/g, '_')
    const stored: StoredAccount = {
      id: newAccountId(),
      server: d.server,
      loginName: d.user,
      userId: d.user,
      name: d.user,
      key: '',
      // Their own cache: never mix two accounts' messages.
      dir: join('talk-dev', `${host}-${d.user}`),
      secret: '',
    }
    list.accounts.push(stored)
    const rt = createRuntime(stored, d.password)
    // From the cache of an earlier run, so the account line reads right from the start.
    stored.name = rt.session.displayName || d.user
    bootRuntime(rt, d.password)
  }
  activeId = list.accounts[0]?.id ?? null
  updateBadges()
}

// --- Notifications (docs/notifications.md) -----------------------------------------

/**
 * Every notification on screen. Held on purpose: under Linux an object that was collected
 * loses its click, and all of them close when the app quits.
 */
const shown = new Set<Notification>()
const MAX_SHOWN = 100
/** At most one per account and chat, `<account>|<token>`: a new message replaces it. */
const chatNotices = new Map<string, Notification>()
/** „Neue Nachrichten in {n} Chats“ of an account, and the chats it stands for. */
const summaries = new Map<string, { note: Notification; tokens: Set<string> }>()

function hold(n: Notification): Notification {
  shown.add(n)
  n.on('close', () => shown.delete(n))
  // Not every system says when a notification went away: the oldest one gives way.
  if (shown.size > MAX_SHOWN) shown.delete(shown.values().next().value!)
  return n
}

function dismiss(n: Notification): void {
  shown.delete(n)
  n.close()
}

/**
 * A notification of an account: from two accounts on it names the account, as subtitle on
 * macOS, elsewhere behind the title. A click switches to the account and opens the chat.
 */
function notify(rt: Runtime, options: Electron.NotificationConstructorOptions, token: string | null): Notification {
  const line = accountLine(rt)
  const n = hold(
    new Notification(
      !line ? options : process.platform === 'darwin' ? { ...options, subtitle: line } : { ...options, title: `${options.title} – ${line}` },
    ),
  )
  n.on('click', () => openFromNotification(rt, token))
  n.show()
  return n
}

function openFromNotification(rt: Runtime, token: string | null): void {
  showWindow()
  if (!runtimes.has(rt.id)) return
  const refused = switchTo(rt.id)
  if (refused) send({ type: 'notice', text: refused } satisfies AppEvent)
  else if (token) sendFrom(rt, { type: 'openRoom', token })
}

/** The text below the chat name: as in the chat list, in groups with the author. */
function noticeText(item: MessageNotice): string {
  const m = item.message
  const text = m ? withAttachment(m.text, m.attachment, m.location) : ''
  if (!m || !text) return 'Neue Nachricht'
  return item.isGroup && !m.isSystem && m.author ? `${m.author}: ${text}` : text
}

/**
 * `messageNotices` of the core, for every account. The core also reports the chat that is
 * open: only that one, in the active account with the window in front, stays quiet.
 */
function notifyMessages(rt: Runtime, items: MessageNotice[], sinceStart: boolean): void {
  if (!Notification.isSupported()) return
  const open = (win?.isFocused() ?? false) && rt.id === activeId ? rt.openToken : null
  const fresh = items.filter((i) => i.token !== open)
  if (fresh.length === 0) return
  // Many at once, or what came in while the app was closed: one notification for all.
  if (fresh.length > 1 && (sinceStart || items.length > 3)) {
    notifySummary(rt, fresh)
    return
  }
  for (const item of fresh) {
    const key = `${rt.id}|${item.token}`
    const before = chatNotices.get(key)
    if (before) dismiss(before)
    const n = notify(rt, { title: item.roomName, body: noticeText(item), silent: false }, item.token)
    chatNotices.set(key, n)
    n.on('close', () => {
      if (chatNotices.get(key) === n) chatNotices.delete(key)
    })
  }
}

function notifySummary(rt: Runtime, items: MessageNotice[]): void {
  // It stands for these chats now, also for what was shown of them before.
  closeNotices(rt, items.map((i) => i.token))
  const before = summaries.get(rt.id)
  if (before) dismiss(before.note)
  const note = notify(rt, { title: 'Wolkenfunk', body: `Neue Nachrichten in ${items.length} Chats`, silent: false }, null)
  summaries.set(rt.id, { note, tokens: new Set(items.map((i) => i.token)) })
  note.on('close', () => {
    if (summaries.get(rt.id)?.note === note) summaries.delete(rt.id)
  })
}

/**
 * These chats are read, archived, gone or being opened: their notifications close, and the
 * summary with its last chat. `null`: everything of the account (signed out).
 */
function closeNotices(rt: Runtime, tokens: string[] | null): void {
  const summary = summaries.get(rt.id)
  if (tokens === null) {
    for (const [key, n] of chatNotices) {
      if (!key.startsWith(`${rt.id}|`)) continue
      chatNotices.delete(key)
      dismiss(n)
    }
  } else {
    for (const token of tokens) {
      const key = `${rt.id}|${token}`
      const n = chatNotices.get(key)
      if (n) {
        chatNotices.delete(key)
        dismiss(n)
      }
      summary?.tokens.delete(token)
    }
  }
  if (summary && (tokens === null || summary.tokens.size === 0)) {
    summaries.delete(rt.id)
    dismiss(summary.note)
  }
}

/** Quitting: nothing stays behind whose click would lead nowhere. */
function closeAllNotifications(): void {
  chatNotices.clear()
  summaries.clear()
  for (const n of [...shown]) dismiss(n)
}

/** A message finally failed while the user was not looking at its chat. */
function announceFailures(rt: Runtime, token: string, items: PendingMessage[]): void {
  const fresh = items.filter((i) => i.state === 'failed' && !rt.notifiedFailures.has(i.message.referenceId))
  if (fresh.length === 0) return
  fresh.forEach((i) => rt.notifiedFailures.add(i.message.referenceId))
  const visible = (win?.isFocused() ?? false) && rt.id === activeId && rt.openToken === token
  if (visible || !Notification.isSupported()) return
  const room = rt.session.rooms().find((r) => r.token === token)
  notify(rt, { title: 'Nachricht nicht gesendet', body: room?.name ?? 'Unterhaltung' }, token)
}

// --- Calls: media engine bridge (core in this process, WebRTC in the renderer) -----

/** Commands waiting for the renderer's engine (window loading or reloading). */
let mediaQueue: MediaCommand[] = []
/** The renderer's engine is listening on `talk:media`. */
let mediaListening = false
/** Peers created by the core and not closed yet. */
const openPeers = new Set<string>()
/** `startAudio` without `stopAudio` yet: replayed to a reloaded renderer. */
let audioCallId: string | null = null
let micEnabled = true
let cameraEnabled = false
/** The call shown last (state drives ringing attention and close-to-tray). */
let call: CallView | null = null
/** The account of that call: controls and media reports go there (docs/accounts.md „Anrufe“). */
let callOwner: Runtime | null = null
/** The account whose core issued the last media command: its peers, its reports. */
let mediaOwner: Runtime | null = null
let ringNotification: Notification | null = null

function onMediaCommand(rt: Runtime, cmd: MediaCommand): void {
  mediaOwner = rt
  switch (cmd.cmd) {
    case 'createPeer':
      openPeers.add(cmd.peerId)
      break
    case 'closePeer':
      openPeers.delete(cmd.peerId)
      break
    case 'startAudio':
      audioCallId = cmd.callId
      break
    case 'stopAudio':
      if (audioCallId === cmd.callId) {
        audioCallId = null
        cameraEnabled = false
      }
      break
    case 'setMicrophoneEnabled':
      micEnabled = cmd.enabled
      break
    case 'setCameraEnabled':
      cameraEnabled = cmd.enabled
      break
  }
  // A single channel keeps the core's order; queued commands go out first.
  if (mediaListening && win && !win.isDestroyed()) win.webContents.send('talk:media', cmd)
  else mediaQueue.push(cmd)
}

/** The renderer (and with it every peer connection) went away: reload, crash, window closed. */
function mediaEngineLost(): void {
  if (!mediaListening) return
  mediaListening = false
  const queued = new Set(mediaQueue.flatMap((c) => (c.cmd === 'createPeer' ? [c.peerId] : [])))
  for (const peerId of [...openPeers]) {
    if (queued.has(peerId)) continue
    // The core replaces failed peers with new ones (new ids); late commands for these are ignored.
    openPeers.delete(peerId)
    mediaOwner?.session.mediaReport({ type: 'peerState', peerId, state: 'failed' })
  }
  // The screen track went with the renderer; a reload does not bring the share back.
  const owner = callOwner
  if (owner?.session.currentCall()?.screenSharing) owner.session.mediaReport({ type: 'screenState', on: false })
}

function mediaEngineReady(): void {
  if (!win || win.isDestroyed()) return
  const replay: MediaCommand[] = []
  if (audioCallId && !mediaQueue.some((c) => c.cmd === 'startAudio')) {
    replay.push({ cmd: 'startAudio', callId: audioCallId }, { cmd: 'setMicrophoneEnabled', enabled: micEnabled })
    // No ensureCamera here: it was granted when switched on (no dialog without a click).
    if (cameraEnabled && !mediaQueue.some((c) => c.cmd === 'setCameraEnabled')) replay.push({ cmd: 'setCameraEnabled', enabled: true })
  }
  const pending = [...replay, ...mediaQueue]
  mediaQueue = []
  mediaListening = true
  for (const cmd of pending) win.webContents.send('talk:media', cmd)
}

/** In a call (or ringing): the window must not go away, the renderer holds the media. */
function callActive(): boolean {
  return call !== null && call.state !== 'ended'
}

/** A call that rings no more: no switching to another account meanwhile. */
function callRunning(): boolean {
  return callActive() && call?.state !== 'incoming'
}

/** The account of the running call; throws without one. */
function callRuntime(): Runtime {
  if (!callOwner || !runtimes.has(callOwner.id)) throw new Error('Kein Anruf')
  return callOwner
}

function onCallChanged(rt: Runtime, next: CallView | null): void {
  // The core keeps one call per process: `null` from another account is not about this one.
  if (callOwner && callOwner !== rt && !next) return
  const wasRinging = call?.state === 'incoming'
  const wasActive = callActive()
  call = next
  callOwner = next ? rt : null
  // Keep awake, network, and on macOS dock, menu bar and the notification with buttons.
  callChanged(next ? { ...next, accountLine: accountLine(rt) } : null)
  const ringing = next?.state === 'incoming'
  if (callActive() !== wasActive) updateTrayMenu()
  if (ringing && !wasRinging) startRingAttention(rt, next)
  else if (!ringing && wasRinging) stopRingAttention()
  // Whichever account is shown: the call UI belongs to the window.
  send({ type: 'callChanged', call: next, accountId: rt.id })
}

/** The call's account went away (signed out): the call UI closes. */
function callEnded(): void {
  const rt = callOwner
  if (rt) onCallChanged(rt, null)
}

/** Incoming call: blink the taskbar entry and show a silent notification (the bar rings). */
function startRingAttention(rt: Runtime, c: CallView): void {
  if (win && !win.isDestroyed() && !win.isFocused()) win.flashFrame(true)
  // macOS: system.ts shows the notification with Annehmen/Ablehnen buttons.
  if (process.platform === 'darwin' || !Notification.isSupported()) return
  if (ringNotification) dismiss(ringNotification)
  const line = accountLine(rt)
  const title = c.isGroup ? `Gruppenanruf · ${c.roomName}` : c.roomName
  const n = hold(new Notification({
    title: line ? `${title} – ${line}` : title,
    body: c.isGroup && c.callerName ? `${c.callerName} ruft an` : 'Eingehender Anruf',
    silent: true,
    urgency: 'critical',
    timeoutType: 'never',
  }))
  // The bar in the window answers; the chat opens only where no call blocks the switch.
  n.on('click', () => {
    showWindow()
    if (rt.id === activeId) sendFrom(rt, { type: 'openRoom', token: c.token })
  })
  n.show()
  ringNotification = n
}

function notifyMissedCall(rt: Runtime, m: { token: string; roomName: string; callerName: string; isGroup: boolean }): void {
  if (!Notification.isSupported()) return
  notify(rt, { title: 'Verpasster Anruf', body: m.isGroup ? `${m.callerName || 'Gruppenanruf'} · ${m.roomName}` : `von ${m.roomName}` }, m.token)
}

/** A recording, transcript or summary is ready while the app runs: a quiet notification. */
function notifyRecordings(rt: Runtime, items: RecordingNotice[]): void {
  // The core sends no first (empty) list, so "the first event is old" does not hold;
  // the server timestamp decides (a minute of slack for clock skew).
  const fresh = items.filter((i) => !rt.knownNotices.has(i.id) && i.timestamp >= rt.noticesSince - 60)
  items.forEach((i) => rt.knownNotices.add(i.id))
  if (!Notification.isSupported()) return
  for (const i of fresh) {
    notify(rt, { title: recordingNoticeTitle(i), body: i.fileName ? `${i.roomName} · ${i.fileName}` : i.roomName, silent: true }, i.token)
  }
}

function stopRingAttention(): void {
  if (win && !win.isDestroyed()) win.flashFrame(false)
  if (ringNotification) dismiss(ringNotification)
  ringNotification = null
}

/** „Annehmen“ (bar, notification, dock): first in the call's account, then switch there. */
async function acceptCall(token: string): Promise<void> {
  const rt = callRuntime()
  if (!(await ensureMicrophone('call'))) return
  await rt.session.acceptCall(token)
  if (runtimes.has(rt.id)) switchTo(rt.id, true)
}

// --- Attachments: staging ----------------------------------------------------------

function stagingDir(rt: Runtime): string {
  const dir = join(rt.dir, 'staging')
  mkdirSync(dir, { recursive: true })
  return dir
}

/** File name without path parts or characters that confuse file systems. */
function safeName(name: string): string {
  const clean = basename(name).replace(/[\\/:*?"<>|\x00-\x1f]/g, '_').trim()
  return clean || 'Datei'
}

function stagedPath(rt: Runtime, name: string): string {
  return join(stagingDir(rt), `${randomBytes(8).toString('hex')}${extname(safeName(name)).toLowerCase()}`)
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
  '.m4a': 'audio/mp4',
  '.aac': 'audio/aac',
  '.wav': 'audio/wav',
  '.opus': 'audio/ogg',
  '.oga': 'audio/ogg',
  '.ogg': 'audio/ogg',
  '.txt': 'text/plain',
  '.zip': 'application/zip',
}

function mimeFor(name: string, given: string): string {
  const ext = extname(name).toLowerCase()
  // A contact (docs/attachments.md): the server calls every `.vcf` a vCard, whatever the
  // system says, and the pending bubble shows the contact tile from this type.
  if (ext === '.vcf') return 'text/vcard'
  return given || MIME_BY_EXT[ext] || 'application/octet-stream'
}

/**
 * Audio type from the file's first bytes, `fallback` when unknown. The official Talk apps send
 * AAC/MP4 named `.mp3`; files without a (known) extension would otherwise lose range requests.
 */
async function sniffAudio(path: string, fallback: string): Promise<string> {
  const fh = await open(path, 'r').catch(() => null)
  if (!fh) return fallback
  try {
    const b = Buffer.alloc(12)
    const { bytesRead } = await fh.read(b, 0, 12, 0)
    if (bytesRead < 12) return fallback
    const ascii = (from: number, to: number): string => b.toString('latin1', from, to)
    if (ascii(4, 8) === 'ftyp') return 'audio/mp4'
    if (ascii(0, 4) === 'RIFF' && ascii(8, 12) === 'WAVE') return 'audio/wav'
    if (ascii(0, 4) === 'OggS') return 'audio/ogg'
    if (b.readUInt32BE(0) === 0x1a45dfa3) return 'audio/webm'
    if (ascii(0, 3) === 'ID3') return 'audio/mpeg'
    if (b[0] === 0xff && (b[1] & 0xe0) === 0xe0) {
      // Layer bits 00 mark AAC in ADTS, anything else is MPEG audio.
      return (b[1] & 0x06) === 0 ? 'audio/aac' : 'audio/mpeg'
    }
    return fallback
  } finally {
    await fh.close()
  }
}

async function stageData(rt: Runtime, name: string, mime: string, data: Uint8Array, width: number, height: number): Promise<StagedFile> {
  const path = stagedPath(rt, name)
  await writeFile(path, data)
  return { path, name: safeName(name), mime: mimeFor(name, mime), size: data.byteLength, width, height }
}

async function stagePath(rt: Runtime, source: string, name: string, mime: string): Promise<StagedFile> {
  const info = await stat(source)
  if (!info.isFile()) throw new Error('Ordner können nicht gesendet werden')
  const path = stagedPath(rt, name || source)
  // A copy: the outbox moves its file, the user's original must stay.
  await copyFile(source, path)
  const fileName = safeName(name || basename(source))
  return { path, name: fileName, mime: mimeFor(fileName, mime), size: info.size, width: 0, height: 0 }
}

// --- Tray (Linux/Windows; macOS keeps the dock) -------------------------------------

let tray: Tray | null = null
/** Chats with something new over all accounts, as tray and taskbar show it. */
let unread = 0

/**
 * Monochrome glyph, with a dot while something is unread. Windows says whether the taskbar
 * is dark; Linux does not, and GNOME, KDE and Ubuntu panels mostly are.
 */
function trayIcon(): string {
  const darkPanel = process.platform === 'win32' ? nativeTheme.shouldUseDarkColorsForSystemIntegratedUI : true
  return resource(`tray-on-${darkPanel ? 'dark' : 'light'}${unread > 0 ? '-dot' : ''}.png`)
}

function createTray(): void {
  if (process.platform === 'darwin' || tray) return
  try {
    tray = new Tray(trayIcon())
  } catch (err) {
    console.warn('no tray:', (err as Error).message)
    return
  }
  tray.setToolTip(trayToolTip())
  tray.on('click', showWindow)
  updateTrayMenu()
  nativeTheme.on('updated', () => tray?.setImage(trayIcon()))
}

function trayToolTip(): string {
  return unread > 0 ? 'Wolkenfunk, neue Nachrichten' : 'Wolkenfunk'
}

/**
 * The sum of `badge().chats` (docs/notifications.md „Desktop“): the tray picture changes
 * only between „nothing“ and „something“, the Windows counter with the number.
 */
function showUnread(total: number): void {
  const before = unread
  unread = total
  if (total === before) return
  if ((total > 0) !== (before > 0)) {
    tray?.setImage(trayIcon())
    tray?.setToolTip(trayToolTip())
    // macOS: the menu bar icon.
    unreadChanged(total > 0)
  }
  showTaskbarCount()
}

/** Windows: the number of chats with something new on the taskbar button, „9+“ above nine. */
function showTaskbarCount(): void {
  if (process.platform !== 'win32' || !win || win.isDestroyed()) return
  if (unread <= 0) {
    win.setOverlayIcon(null, '')
    return
  }
  const image = nativeImage.createFromPath(resource(`tray-count-${unread > 9 ? 'more' : unread}.png`))
  win.setOverlayIcon(image, unread === 1 ? '1 Chat mit Neuem' : `${unread} Chats mit Neuem`)
}

function updateTrayMenu(): void {
  tray?.setContextMenu(
    Menu.buildFromTemplate([
      { label: 'Wolkenfunk öffnen', click: showWindow },
      ...(callActive() ? [{ label: 'Auflegen', click: () => callOwner?.session.hangUp() }] : []),
      { type: 'separator' },
      {
        label: 'Beim Schließen im Hintergrund weiterlaufen',
        type: 'checkbox',
        checked: settings.closeToTray,
        click: (item) => setCloseToTray(item.checked),
      },
      { type: 'separator' },
      { label: 'Beenden', click: () => app.quit() },
    ]),
  )
}

/**
 * „Beim Anmelden starten“: the setting is ours, the entry is the system's. Under Linux it is
 * written again at every start, so it follows an AppImage that was moved or replaced.
 */
function applyOpenAtLogin(): void {
  if (!autostartAvailable()) return
  try {
    setAutostart(settings.openAtLogin)
  } catch (err) {
    console.warn('autostart entry failed:', (err as Error).message)
  }
}

/** Tray check mark and the switch in „Allgemein“ are the same setting. */
function setCloseToTray(on: boolean): void {
  settings.closeToTray = on
  saveSettings()
  updateTrayMenu()
  send({ type: 'closeToTrayChanged', on } satisfies AppEvent)
}

// --- Appearance (docs/settings.md „Darstellung“) ----------------------------------

/** Follows the CSS colour scheme, the window background, the tray icon and the avatars. */
function applyTheme(): void {
  nativeTheme.themeSource = settings.theme
  win?.setBackgroundColor(windowBackground())
}

function windowBackground(): string {
  return nativeTheme.shouldUseDarkColors ? '#121212' : '#ffffff'
}

function appearance(): Appearance {
  return { theme: settings.theme, accent: settings.accent, layout: settings.layout, splitChats: settings.splitChats }
}

function appearanceChanged(): void {
  saveSettings()
  send({ type: 'appearanceChanged', ...appearance() } satisfies AppEvent)
}

function setAppearance(theme: Theme, accent: string): void {
  if (!THEMES.includes(theme) || !ACCENTS.some((a) => a.id === accent)) return
  settings.theme = theme
  settings.accent = accent
  applyTheme()
  appearanceChanged()
}

function setLayout(layout: Layout): void {
  if (!LAYOUTS.includes(layout)) return
  settings.layout = layout
  appearanceChanged()
}

function setSplitChats(on: boolean): void {
  settings.splitChats = on === true
  appearanceChanged()
}

/** Opens the settings in the window (⌘, in the macOS app menu). */
function openSettings(): void {
  showWindow()
  send({ type: 'openSettings' } satisfies AppEvent)
}

// --- Window ------------------------------------------------------------------------

/** Only the first window of a run that the system started at login stays hidden. */
let startHidden = false

function showWindow(): void {
  // macOS keeps running without a window: a notification click or a second launch reopens it.
  if (!win) return createWindow()
  if (win.isMinimized()) win.restore()
  win.show()
  win.focus()
  // macOS: a button in a notification („Annehmen“) does not activate the app, and focus()
  // alone leaves an inactive app behind the others. Every caller answers a click.
  if (process.platform === 'darwin') app.focus({ steal: true })
}

function createWindow(): void {
  win = new BrowserWindow({
    width: 1100,
    height: 760,
    minWidth: 380,
    minHeight: 480,
    show: false,
    title: 'Wolkenfunk',
    autoHideMenuBar: true,
    backgroundColor: windowBackground(),
    webPreferences: {
      preload: join(__dirname, '../preload/index.js'),
      sandbox: true,
      contextIsolation: true,
      spellcheck: true,
      // Calls live in this renderer: ringing, tones and media must not be throttled when hidden.
      backgroundThrottling: false,
    },
  })
  // Started at login: the app waits in the tray (macOS: in the dock) until it is opened.
  const hidden = startHidden
  startHidden = false
  if (!hidden) win.once('ready-to-show', () => win?.show())
  win.on('show', showTaskbarCount)
  win.on('focus', () => {
    win?.flashFrame(false)
    // Back in front (docs/connection.md): every account fetches its list, and a retry
    // that is waiting runs now. The core folds a burst into one request.
    runtimes.forEach((r) => r.session.networkChanged())
  })
  // However it was entered (call view, green button, F11, menu): the call view shows it.
  win.on('enter-full-screen', () => send({ type: 'windowFullscreen', on: true } satisfies AppEvent))
  win.on('leave-full-screen', () => send({ type: 'windowFullscreen', on: false } satisfies AppEvent))
  win.on('close', (e) => {
    // Keeps running in the tray so calls can ring (and a running call keeps its media).
    // macOS: always, like every Mac app – Cmd+Q quits, the dock icon brings it back.
    if (quitting) return
    if (process.platform === 'darwin' || callActive() || (settings.closeToTray && tray)) {
      e.preventDefault()
      const w = win
      // Leaving fullscreen first, otherwise macOS leaves a black space behind.
      if (w?.isFullScreen()) {
        w.once('leave-full-screen', () => w.hide())
        w.setFullScreen(false)
      } else w?.hide()
    }
  })
  win.on('closed', () => {
    finishRecording()
    mediaEngineLost()
    win = null
  })
  // Renderer crashed or reloaded mid-recording: what was recorded stays as draft.
  win.webContents.on('render-process-gone', () => {
    finishRecording()
    mediaEngineLost()
  })
  win.webContents.on('did-start-loading', () => {
    finishRecording()
    mediaEngineLost()
  })

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

/**
 * A call for one account: the renderer passes the id of the account it shows, and the call
 * runs in exactly that account, also when another one became active meanwhile
 * (docs/accounts.md). `r` is null when the account is gone.
 */
function handle<A extends unknown[]>(channel: string, fn: (r: Runtime | null, ...args: A) => unknown): void {
  ipcMain.handle(channel, (_e, id: string | null, ...args: A) => fn(id ? (runtimes.get(id) ?? null) : null, ...args))
}

/** The account of a call that cannot do without one. */
function need(r: Runtime | null): Runtime {
  if (!r) throw new Error('Dieses Konto ist nicht mehr angemeldet')
  return r
}

function registerIpc(): void {
  // Accounts (docs/accounts.md): not bound to the account shown.
  ipcMain.handle('talk:account', () => {
    const rt = activeRuntime()
    return rt ? { ...shownAccount(rt), rooms: rt.session.rooms() } : null
  })
  ipcMain.handle('talk:accounts', () => accountsView())
  ipcMain.handle('talk:switchAccount', (_e, id: string) => switchTo(id))
  ipcMain.handle('talk:addAccount', (_e, server: string) => addAccount(server))
  ipcMain.handle('talk:cancelLogin', () => {
    loginFlow?.cancel()
    loginFlow = null
  })
  handle('talk:logout', async (r) => {
    if (r) await removeAccount(r, true)
  })

  // From the core cache before the first request completes.
  handle('talk:rooms', (r) => r?.session.rooms() ?? [])
  handle('talk:cachedChat', (r, token: string) => need(r).session.cachedChat(token))
  handle('talk:openChat', (r, token: string) => {
    need(r).openToken = token
    closeNotices(r!, [token])
    return r!.session.openChat(token)
  })
  handle('talk:closeChat', (r, token: string) => {
    if (!r) return
    if (r.openToken === token) r.openToken = null
    r.session.closeChat(token)
  })
  handle('talk:loadOlder', (r, token: string, beforeId: number) => need(r).session.loadOlder(token, beforeId))
  // docs/search.md „Nicht gefunden“: the two reasons as the texts the chat shows.
  handle('talk:messagesAround', (r, token: string, messageId: number) =>
    need(r)
      .session.messagesAround(token, messageId)
      .catch((e: unknown) => ({ error: String(e).includes('NETWORK') ? 'Keine Verbindung' : 'Nachricht nicht gefunden' })),
  )
  handle('talk:loadNewer', (r, token: string, afterId: number) => need(r).session.loadNewer(token, afterId))
  handle('talk:searchMessages', (r, query: string, token: string | null, limit: number) =>
    need(r).session.searchMessages(query, token, limit),
  )
  handle('talk:searchMessagesEverywhere', (r, query: string, token: string | null, limit: number) =>
    need(r).session.searchMessagesEverywhere(query, token, limit),
  )
  handle('talk:refreshRooms', (r) => r?.session.refreshRooms().catch(() => undefined))
  handle('talk:enqueueText', (r, token: string, text: string, mentions: MentionPick[], replyTo: ReplyPreview | null) =>
    need(r).session.enqueueText(token, text, mentions, replyTo, core.newReferenceId()),
  )
  handle(
    'talk:enqueueFile',
    (r, token: string, file: StagedFile, caption: string, mentions: MentionPick[], replyTo: ReplyPreview | null) => {
      const rt = need(r)
      if (!inside(join(rt.dir, 'staging'), file.path)) throw new Error('not a staged file')
      return rt.session.enqueueFile(
        token,
        file.path,
        file.name,
        file.mime,
        file.width,
        file.height,
        caption,
        mentions,
        replyTo,
        core.newReferenceId(),
      )
    },
  )
  handle('talk:pending', (r, token: string) => r?.session.pending(token) ?? [])
  handle('talk:retrySend', (r, referenceId: string) => {
    r?.notifiedFailures.delete(referenceId)
    r?.session.retrySend(referenceId)
  })
  handle('talk:discard', (r, referenceId: string) => r?.session.discard(referenceId))
  // Every account's outbox waits for the network.
  ipcMain.on('talk:networkChanged', () => runtimes.forEach((r) => r.session.networkChanged()))
  ipcMain.on('talk:networkLost', () => runtimes.forEach((r) => r.session.networkLost()))
  handle('talk:stageData', (r, name: string, mime: string, data: Uint8Array, width: number, height: number) =>
    stageData(need(r), name, mime, data, width, height),
  )
  handle('talk:stagePath', (r, path: string, name: string, mime: string) => stagePath(need(r), path, name, mime))
  handle('talk:unstage', async (r, path: string) => {
    if (r && inside(join(r.dir, 'staging'), path)) await rm(path, { force: true })
  })
  handle('talk:voiceStart', async (r, token: string, sampleRate: number) => {
    const rt = need(r)
    finishRecording()
    if (!(await ensureMicrophone('voice'))) throw new Error('Kein Zugriff auf das Mikrofon')
    recording = { rt, token, rec: rt.session.startVoice(token, sampleRate) }
  })
  // The recording knows its account: these follow it, whichever account is shown.
  ipcMain.handle('talk:voicePush', (_e, pcm: Uint8Array) => {
    if (!recording) throw new Error('no recording')
    return recording.rec.push(Buffer.from(pcm.buffer, pcm.byteOffset, pcm.byteLength))
  })
  ipcMain.handle('talk:voiceFinish', () => finishRecording())
  ipcMain.handle('talk:voiceCancel', () => {
    recording?.rec.cancel()
    recording = null
  })
  handle('talk:voiceDraft', (r, token: string) =>
    !r || (recording?.rt === r && recording.token === token) ? null : (r.session.voiceDraft(token) ?? null),
  )
  handle('talk:voiceDiscard', (r, token: string) => r?.session.discardVoiceDraft(token))
  handle('talk:voiceSend', (r, token: string, replyTo: ReplyPreview | null) =>
    need(r).session.sendVoiceDraft(token, voiceFileName(), replyTo, core.newReferenceId()),
  )
  handle('talk:fetchFile', (r, path: string, size: number) => need(r).session.fetchFile(path, size))
  handle('talk:audioInfo', (r, localPath: string) => {
    const rt = need(r)
    if (!inside(rt.dir, localPath)) throw new Error('not an app file')
    return rt.session.audioInfo(localPath)
  })
  handle('talk:markRead', (r, token: string, messageId: number) => r?.session.markRead(token, messageId))
  handle('talk:editSource', (r, token: string, messageId: number) => (r ? r.session.editSource(token, messageId) : null))
  handle('talk:editMessage', (r, token: string, messageId: number, text: string, mentions: MentionPick[]) =>
    r ? r.session.editMessage(token, messageId, text, mentions) : NOT_SIGNED_IN,
  )
  // docs/mentions.md: answers from memory; without a session or a list there is nobody.
  handle('talk:mentionCandidates', (r, token: string, query: string, limit: number) =>
    r ? r.session.mentionCandidates(token, query, limit).catch(() => []) : [],
  )
  handle('talk:existingDirect', (r, userId: string) => r?.session.existingDirect(userId) ?? null)
  handle('talk:deleteMessage', (r, token: string, messageId: number) =>
    r ? r.session.deleteMessage(token, messageId) : NOT_SIGNED_IN,
  )
  handle('talk:setReaction', (r, token: string, messageId: number, emoji: string | null) =>
    r ? r.session.setReaction(token, messageId, emoji) : NOT_SIGNED_IN,
  )
  handle('talk:reactions', (r, token: string, messageId: number) => r?.session.reactions(token, messageId) ?? [])

  // Groups (docs/groups.md) and categories (docs/categories.md): changes resolve to a
  // German error text or null.
  handle('talk:roomSections', (r, query: string, activeToken: string | null, split: boolean) =>
    r ? r.session.roomSections(query, activeToken, split === true) : [],
  )
  handle('talk:toggleSection', (r, id: string) => (r ? r.session.toggleSection(id) : NOT_SIGNED_IN))
  handle('talk:sortOrder', (r) => r?.session.sortOrder() ?? 'activity')
  handle('talk:setSortOrder', (r, order: string) => (r ? r.session.setSortOrder(order) : NOT_SIGNED_IN))
  handle('talk:categoriesSupported', (r) => r?.session.categoriesSupported() ?? false)
  handle('talk:categories', (r) => (r ? r.session.categories() : []))
  handle('talk:createCategory', (r, name: string, tokens: string[]) =>
    r ? r.session.createCategory(name, tokens) : { id: null, error: NOT_SIGNED_IN },
  )
  handle('talk:renameCategory', (r, id: string, name: string) => (r ? r.session.renameCategory(id, name) : NOT_SIGNED_IN))
  handle('talk:deleteCategory', (r, id: string) => (r ? r.session.deleteCategory(id) : NOT_SIGNED_IN))
  handle('talk:moveCategory', (r, id: string, offset: number) => (r ? r.session.moveCategory(id, offset) : NOT_SIGNED_IN))
  handle('talk:setCategoryRooms', (r, id: string, tokens: string[]) => (r ? r.session.setCategoryRooms(id, tokens) : NOT_SIGNED_IN))
  const bytes = (data: Uint8Array | null) => (data ? Buffer.from(data) : null)
  handle('talk:canCreateGroups', (r) => r?.session.canCreateGroups() ?? false)
  handle('talk:searchUsers', (r, query: string, excludeToken: string | null) => need(r).session.searchUsers(query, excludeToken))
  handle('talk:openDirect', (r, userId: string) => (r ? r.session.openDirect(userId) : { token: null, error: NOT_SIGNED_IN }))
  handle('talk:createGroup', (r, name: string, userIds: string[], photo: Uint8Array | null) =>
    r ? r.session.createGroup(name, userIds, bytes(photo)) : { token: null, error: NOT_SIGNED_IN },
  )
  handle('talk:members', (r, token: string) => need(r).session.members(token))
  handle('talk:addMembers', (r, token: string, userIds: string[]) => (r ? r.session.addMembers(token, userIds) : NOT_SIGNED_IN))
  handle('talk:removeMember', (r, token: string, attendeeId: number) =>
    r ? r.session.removeMember(token, attendeeId) : NOT_SIGNED_IN,
  )
  handle('talk:setModerator', (r, token: string, attendeeId: number, moderator: boolean) =>
    r ? r.session.setModerator(token, attendeeId, moderator) : NOT_SIGNED_IN,
  )
  handle('talk:renameGroup', (r, token: string, name: string) => (r ? r.session.renameGroup(token, name) : NOT_SIGNED_IN))
  handle('talk:setDescription', (r, token: string, description: string) =>
    r ? r.session.setDescription(token, description) : NOT_SIGNED_IN,
  )
  handle('talk:setGroupPhoto', (r, token: string, photo: Uint8Array | null) =>
    r ? r.session.setGroupPhoto(token, bytes(photo)) : NOT_SIGNED_IN,
  )
  handle('talk:setGroupLink', (r, token: string, on: boolean) => (r ? r.session.setGroupLink(token, on) : NOT_SIGNED_IN))
  handle('talk:groupLink', (r, token: string) => need(r).session.groupLink(token))
  handle('talk:setNotificationLevel', (r, token: string, level: number) =>
    r ? r.session.setNotificationLevel(token, level) : NOT_SIGNED_IN,
  )
  handle('talk:leaveNeedsModerator', (r, token: string) => need(r).session.leaveNeedsModerator(token))
  handle('talk:leaveGroup', (r, token: string) => (r ? r.session.leaveGroup(token) : NOT_SIGNED_IN))
  handle('talk:deleteGroup', (r, token: string) => (r ? r.session.deleteGroup(token) : NOT_SIGNED_IN))

  // Chat menu (docs/settings.md): shown at once, a German error text or null.
  handle('talk:setFavorite', (r, token: string, on: boolean) => (r ? r.session.setFavorite(token, on) : NOT_SIGNED_IN))
  handle('talk:setRoomCategories', (r, token: string, ids: string[]) =>
    r ? r.session.setRoomCategories(token, ids) : NOT_SIGNED_IN,
  )
  handle('talk:markRoomRead', (r, token: string) => (r ? r.session.markRoomRead(token) : NOT_SIGNED_IN))

  // Settings (docs/settings.md): device settings in settings.json.
  ipcMain.handle('talk:appearance', () => appearance())
  ipcMain.handle('talk:setAppearance', (_e, theme: Theme, accent: string) => setAppearance(theme, accent))
  ipcMain.handle('talk:setLayout', (_e, layout: Layout) => setLayout(layout))
  ipcMain.handle('talk:setSplitChats', (_e, on: boolean) => setSplitChats(on))
  ipcMain.handle('talk:closeToTray', () => settings.closeToTray)
  ipcMain.handle('talk:setCloseToTray', (_e, on: boolean) => setCloseToTray(!!on))
  ipcMain.handle('talk:openAtLogin', () => {
    if (!autostartAvailable()) return null
    const on = autostartOn(settings.openAtLogin)
    if (on !== settings.openAtLogin) {
      settings.openAtLogin = on
      saveSettings()
    }
    return on
  })
  ipcMain.handle('talk:setOpenAtLogin', (_e, on: boolean) => {
    settings.openAtLogin = !!on
    saveSettings()
    applyOpenAtLogin()
  })
  ipcMain.handle('talk:appInfo', () => ({
    version: app.getVersion(),
    core: core.coreVersion(),
    canUpdate: updatesActive(),
  }))
  ipcMain.handle('talk:checkForUpdates', () => checkForUpdatesNow())

  // Calls (docs/calls.md). Starting is the shown account's; everything after that goes to
  // the account of the call (docs/accounts.md „Anrufe“).
  // macOS asks for the microphone first (and explains a refusal) instead of a silent call.
  handle('talk:startCall', async (r, token: string, silent: boolean) => {
    const rt = need(r)
    if (await ensureMicrophone('call')) await rt.session.startCall(token, silent)
  })
  ipcMain.handle('talk:acceptCall', (_e, token: string) => acceptCall(token))
  ipcMain.handle('talk:declineCall', (_e, token: string) => callOwner?.session.declineCall(token))
  ipcMain.handle('talk:hangUp', () => callOwner?.session.hangUp())
  ipcMain.handle('talk:setCallMuted', (_e, muted: boolean) => callOwner?.session.setCallMuted(muted))
  ipcMain.handle('talk:setFullScreen', (_e, on: boolean) => win?.setFullScreen(on))
  ipcMain.handle('talk:isFullScreen', () => win?.isFullScreen() ?? false)
  // macOS asks for the camera on the first switch-on; refused, it stays off (the dialog explains).
  ipcMain.handle('talk:setCameraEnabled', async (_e, on: boolean) => {
    if (on && !(await ensureCamera())) return false
    callOwner?.session.setCameraEnabled(on)
    return true
  })
  ipcMain.handle('talk:setScreenShare', (_e, on: boolean) => callOwner?.session.setScreenShare(on))
  ipcMain.handle('talk:currentCall', () => {
    const rt = callOwner
    const c = rt?.session.currentCall()
    return rt && c ? { call: c, account: rt.id } : null
  })
  // Recording (docs/calls.md, „Aufnahme“): of the call; the notices are the shown account's.
  ipcMain.handle('talk:startRecording', (_e, video: boolean) => callRuntime().session.startRecording(video))
  ipcMain.handle('talk:stopRecording', () => callRuntime().session.stopRecording())
  ipcMain.handle('talk:consentToRecording', () => callOwner?.session.consentToRecording())
  handle('talk:recordingNotices', (r) => r?.session.recordingNotices() ?? [])
  handle('talk:shareRecording', (r, id: number) => need(r).session.shareRecording(id))
  handle('talk:dismissRecording', (r, id: number) => need(r).session.dismissRecording(id))
  ipcMain.on('talk:mediaReady', (e) => {
    if (e.sender === win?.webContents) mediaEngineReady()
  })
  ipcMain.on('talk:mediaReport', (e, report: MediaReport) => {
    const rt = mediaOwner
    if (e.sender !== win?.webContents || !rt || !runtimes.has(rt.id)) return
    // Reports of peers lost with an earlier renderer must not reach the core.
    if ('peerId' in report && !openPeers.has(report.peerId)) return
    try {
      rt.session.mediaReport(report)
    } catch (err) {
      console.warn('media report rejected:', (err as Error).message)
    }
  })
}

const NOT_SIGNED_IN = 'Nicht angemeldet'

// --- Avatars: talk-avatar://<account>/room/<token>?v=<version>, …/user/<userId> -------

protocol.registerSchemesAsPrivileged([
  { scheme: 'talk-avatar', privileges: { standard: true, secure: true, supportFetchAPI: true } },
  // `stream`: <audio> reads local files in ranges (seeking in long voice messages).
  { scheme: 'talk-media', privileges: { standard: true, secure: true, supportFetchAPI: true, stream: true } },
])

/** `<account>|room|<token>|<version>|<dark>`: tokens of different servers overlap. */
const avatarCache = new Map<string, { data: Buffer; contentType: string }>()

/** `talk-…://<account>/<kind>/<id>`: the account, the kind and the decoded id. */
function parseAppUrl(request: Request): { rt: Runtime | null; url: URL; kind: string; id: string } {
  const url = new URL(request.url)
  const [, kind = '', ...rest] = url.pathname.split('/')
  return { rt: runtimes.get(url.hostname) ?? null, url, kind, id: decodeURIComponent(rest.join('/')) }
}

function registerAvatarProtocol(): void {
  protocol.handle('talk-avatar', async (request) => {
    const { rt, url, kind, id } = parseAppUrl(request)
    if (!rt || (kind !== 'room' && kind !== 'user')) return new Response(null, { status: 404 })
    const dark = nativeTheme.shouldUseDarkColors
    const key = `${rt.id}|${kind}|${id}|${url.searchParams.get('v') ?? ''}|${dark}`
    let hit = avatarCache.get(key)
    if (!hit) {
      try {
        hit = kind === 'user' ? await rt.session.userAvatar(id, dark) : await rt.session.avatar(id, dark)
        avatarCache.set(key, hit)
      } catch {
        return new Response(null, { status: 404 })
      }
    }
    return new Response(new Uint8Array(hit.data), {
      headers: { 'content-type': hit.contentType, 'cache-control': 'max-age=86400' },
    })
  })
}

// --- Media: talk-media://<account>/preview/<fileId>?w=&h=, …/local/file?p=<path> -------

function imageType(data: Uint8Array, fallback: string): string {
  if (data[0] === 0xff && data[1] === 0xd8) return 'image/jpeg'
  if (data[0] === 0x89 && data[1] === 0x50) return 'image/png'
  if (data[0] === 0x47 && data[1] === 0x49) return 'image/gif'
  if (data[8] === 0x57 && data[9] === 0x45) return 'image/webp'
  return fallback
}

function registerMediaProtocol(): void {
  protocol.handle('talk-media', async (request) => {
    const { rt, url, kind, id } = parseAppUrl(request)
    if (!rt) return new Response(null, { status: 404 })
    try {
      if (kind === 'preview') {
        const w = Math.min(Number(url.searchParams.get('w')) || 512, 4096)
        const h = Math.min(Number(url.searchParams.get('h')) || 512, 4096)
        const data = await rt.session.filePreview(id, w, h)
        return new Response(new Uint8Array(data), {
          headers: { 'content-type': imageType(data, 'image/jpeg'), 'cache-control': 'max-age=604800' },
        })
      }
      if (kind === 'local') {
        // Only files of the outbox/staging of this account, nothing else on disk.
        const path = url.searchParams.get('p') ?? ''
        if (!inside(rt.dir, path)) return new Response(null, { status: 403 })
        let type = mimeFor(path, '')
        if (type.startsWith('audio/') || type === 'application/octet-stream') type = await sniffAudio(path, type)
        if (type.startsWith('audio/') || type.startsWith('video/')) return rangeResponse(path, type, request)
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

/** A local media file, honouring `Range` (the media element seeks by range requests). */
async function rangeResponse(path: string, type: string, request: Request): Promise<Response> {
  const size = (await stat(path)).size
  const m = /^bytes=(\d*)-(\d*)$/.exec(request.headers.get('range') ?? '')
  let start = 0
  let end = size - 1
  if (m) {
    if (m[1]) {
      start = Number(m[1])
      if (m[2]) end = Math.min(Number(m[2]), size - 1)
    } else if (m[2]) {
      start = Math.max(0, size - Number(m[2]))
    }
    if (start > end || start >= size) {
      return new Response(null, { status: 416, headers: { 'content-range': `bytes */${size}` } })
    }
  }
  const headers: Record<string, string> = {
    'content-type': type,
    'content-length': String(end - start + 1),
    'accept-ranges': 'bytes',
  }
  if (m) headers['content-range'] = `bytes ${start}-${end}/${size}`
  // Streamed: an hour of audio is not read into memory for every seek.
  const body = Readable.toWeb(createReadStream(path, { start, end })) as ReadableStream<Uint8Array>
  return new Response(body, { status: m ? 206 : 200, headers })
}

let quitting = false
let readyToQuit = false

/** Gives every account's outbox a moment (one shared deadline) before the process ends. */
async function quitAfterOutbox(): Promise<void> {
  if (quitting) return
  quitting = true
  finishRecording()
  closeAllNotifications()
  const all = [...runtimes.values()]
  if (callOwner && callActive()) {
    // Leave the call properly instead of letting the server time us out.
    await callOwner.session.hangUpAndWait(3000).catch(() => undefined)
  }
  const waiting = all.filter((r) => r.session.outboxUnsent() > 0)
  if (waiting.length) {
    await Promise.race([
      Promise.all(waiting.map((r) => r.session.waitOutboxIdle().catch(() => undefined))),
      new Promise((r) => setTimeout(r, 10_000)),
    ])
  }
  for (const r of all) r.session.shutdown()
  readyToQuit = true
  // Never from inside the `before-quit` that started this (nothing was awaited above):
  // Electron drops a nested quit and the app would stay alive without windows.
  setImmediate(() => app.quit())
}

/**
 * Gear for „Einstellungen …“ (system symbol): from macOS 26 on, the system menu items carry
 * icons, and an item without one sits out of line. Earlier versions show no menu icons.
 */
function settingsMenuIcon(): Electron.NativeImage | undefined {
  if (Number(process.getSystemVersion().split('.')[0]) < 26) return undefined
  const image = nativeImage.createFromNamedImage('gearshape')
  if (image.isEmpty()) return undefined
  const icon = image.resize({ height: 16 })
  icon.setTemplateImage(true)
  return icon
}

/** Standard macOS menu bar (app, edit, view, window); other platforms keep no menu. */
function macMenu(): Menu {
  const name = app.getName()
  return Menu.buildFromTemplate([
    {
      label: name,
      submenu: [
        { role: 'about', label: `Über ${name}` },
        { type: 'separator' },
        { label: 'Einstellungen …', accelerator: 'CmdOrCtrl+,', icon: settingsMenuIcon(), click: openSettings },
        { type: 'separator' },
        { role: 'services', label: 'Dienste' },
        { type: 'separator' },
        { role: 'hide', label: `${name} ausblenden` },
        { role: 'hideOthers', label: 'Andere ausblenden' },
        { role: 'unhide', label: 'Alle einblenden' },
        { type: 'separator' },
        { role: 'quit', label: `${name} beenden` },
      ],
    },
    {
      label: 'Bearbeiten',
      submenu: [
        { role: 'undo', label: 'Widerrufen' },
        { role: 'redo', label: 'Wiederholen' },
        { type: 'separator' },
        { role: 'cut', label: 'Ausschneiden' },
        { role: 'copy', label: 'Kopieren' },
        { role: 'paste', label: 'Einsetzen' },
        { role: 'pasteAndMatchStyle', label: 'Einsetzen und Stil anpassen' },
        { role: 'selectAll', label: 'Alles auswählen' },
      ],
    },
    {
      label: 'Darstellung',
      submenu: [
        { role: 'resetZoom', label: 'Originalgröße' },
        { role: 'zoomIn', label: 'Vergrößern' },
        { role: 'zoomOut', label: 'Verkleinern' },
        { type: 'separator' },
        { role: 'togglefullscreen', label: 'Vollbild' },
        ...(app.isPackaged ? [] : [{ role: 'toggleDevTools' as const }]),
      ],
    },
    {
      label: 'Fenster',
      role: 'window',
      submenu: [
        { role: 'minimize', label: 'Im Dock ablegen' },
        { role: 'zoom', label: 'Zoomen' },
        { role: 'close', label: 'Fenster schließen' },
        { type: 'separator' },
        { role: 'front', label: 'Alle nach vorne bringen' },
      ],
    },
  ])
}

// --- App lifecycle -----------------------------------------------------------------

if (!app.requestSingleInstanceLock()) {
  app.quit()
} else {
  app.on('second-instance', showWindow)
  if (process.platform === 'win32') app.setAppUserModelId('org.wolkenfunk.Wolkenfunk')
  // Cmd+Q, the dock menu or logout on macOS: deliver what is in flight first.
  app.on('before-quit', (e) => {
    if (readyToQuit) return
    e.preventDefault()
    void quitAfterOutbox()
  })

  void app.whenReady().then(() => {
    // Before the tray and the first window: no flash of the other theme.
    applyTheme()
    if (process.platform === 'darwin') Menu.setApplicationMenu(macMenu())
    registerIpc()
    registerAvatarProtocol()
    registerMediaProtocol()
    installScreenShare(() => win)
    createTray()
    // Microphone and camera (calls, voice messages) and the screen (only the picture; which
    // one decides the display-media handler in screen.ts) for our window only; nothing else
    // asks for permissions.
    electronSession.defaultSession.setPermissionRequestHandler((wc, permission, callback, details) => {
      const own = wc === win?.webContents
      const types = (details as { mediaTypes?: string[] }).mediaTypes ?? []
      if (permission === 'media') {
        callback(own && types.length > 0 && types.every((t) => t === 'audio' || t === 'video'))
      } else if (permission === 'display-capture') {
        callback(own && types.length > 0 && types.every((t) => t === 'video'))
      } else {
        callback(own && (permission === 'clipboard-sanitized-write' || permission === 'fullscreen'))
      }
    })
    installSystem({
      acceptCall,
      declineCall: (token) => callOwner?.session.declineCall(token),
      hangUp: () => callOwner?.session.hangUp(),
      setCallMuted: (muted) => callOwner?.session.setCallMuted(muted),
      networkChanged: () => runtimes.forEach((r) => r.session.networkChanged()),
      showWindow,
      window: () => (win?.isVisible() ? win : null),
    })
    // A dev run (TALK_DEV_*) starts only its own accounts, never the stored ones.
    const dev = devAccounts()
    if (dev.length) startDevAccounts(dev)
    else startStoredAccounts()
    installAutoUpdate()
    if (process.platform === 'linux' && settings.openAtLogin) applyOpenAtLogin()
    // Without a tray nothing would bring a hidden window back.
    startHidden = startedAtLogin() && (process.platform === 'darwin' || tray !== null)
    createWindow()
    nativeTheme.on('updated', () => avatarCache.clear())
    if (process.platform !== 'darwin') {
      app.on('activate', () => {
        if (BrowserWindow.getAllWindows().length === 0) createWindow()
      })
    }
  })

  app.on('window-all-closed', () => {
    if (process.platform !== 'darwin') void quitAfterOutbox()
  })
}
