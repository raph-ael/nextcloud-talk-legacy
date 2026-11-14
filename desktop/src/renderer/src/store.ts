// Renderer state: conversation list + loaded chats, fed by core events.
// A tiny external store (no framework) consumed via useSyncExternalStore.

import { useSyncExternalStore } from 'react'
import { toast } from './call/store'
import type {
  AccountInfo,
  ChatPage,
  MessageView,
  PendingMessage,
  PendingState,
  ReplyPreview,
  RoomView,
  SectionView,
  SendError,
  StagedFile,
  TalkEvent,
} from '@shared/core'

export type SendStatus = PendingState
/** A message as shown; `status` is set for messages still in the outbox. */
export type UiMessage = MessageView & {
  status?: SendStatus
  error?: SendError | null
  /** Upload progress 0..1, negative when not uploading. */
  progress?: number
  /** Local copy of a pending attachment. */
  localPath?: string | null
}

export interface ChatState {
  messages: UiMessage[]
  hasMore: boolean
  oldestId: number | null
  lastCommonRead: number
  loading: boolean
  loadingOlder: boolean
  /** The server page of this opening has arrived (not only the cache). */
  synced: boolean
}

/**
 * The unread line of the open chat (docs/chat.md, „Ungelesen-Linie“): taken when the chat
 * opens, before anything moves the read marker; gone after an own message or leaving.
 * Place and number come from the loaded messages (`countsAsUnread`), not from counting.
 */
export interface UnreadLine {
  token: string
  /** Messages after this id count (foreign, not system); the line goes above the first one. */
  from: number
  /** The room's unread number when opening: the line shows at least this. */
  base: number
}

/** Whether `m` counts for the unread line starting after `from`. */
export function countsAsUnread(m: UiMessage, from: number): boolean {
  return !m.status && m.id > from && !m.isOwn && m.kind !== 'system'
}

export interface State {
  session: 'loading' | 'loggedOut' | 'ready'
  account: AccountInfo | null
  rooms: RoomView[]
  chats: Record<string, ChatState>
  online: boolean
  activeToken: string | null
  /** „Neue Nachricht“ replaces the conversation list (docs/groups.md). */
  composing: boolean
  /** Token whose group settings replace the chat area. */
  groupInfo: string | null
  /** The chat list in sections (docs/categories.md), for `query` and the open chat. */
  sections: SectionView[]
  /** Search text of the chat list. */
  query: string
  /** The page „Kategorien“ replaces the chat area. */
  categories: boolean
  unreadLine: UnreadLine | null
  /** „Einstellungen“ replace the whole window (docs/settings.md); the page shown on the right. */
  settings: SettingsPage | null
}

export type SettingsPage = 'general' | 'appearance' | 'chats' | 'categories' | 'about'

let state: State = {
  session: 'loading',
  account: null,
  rooms: [],
  chats: {},
  online: true,
  activeToken: null,
  composing: false,
  groupInfo: null,
  sections: [],
  query: '',
  categories: false,
  unreadLine: null,
  settings: null,
}
const listeners = new Set<() => void>()

function set(patch: Partial<State>): void {
  state = { ...state, ...patch }
  listeners.forEach((l) => l())
}

function setChat(token: string, update: (c: ChatState) => ChatState): void {
  const current = state.chats[token] ?? emptyChat()
  set({ chats: { ...state.chats, [token]: update(current) } })
}

function emptyChat(): ChatState {
  return { messages: [], hasMore: true, oldestId: null, lastCommonRead: 0, loading: true, loadingOlder: false, synced: false }
}

export const getState = (): State => state

export function useStore<T>(selector: (s: State) => T): T {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l)
      return () => listeners.delete(l)
    },
    () => selector(state),
  )
}

/** Real messages ascending by id, optimistic ones (not yet confirmed) at the end. */
function merge(existing: UiMessage[], incoming: MessageView[]): UiMessage[] {
  const byId = new Map<number, UiMessage>()
  const pending: UiMessage[] = []
  for (const m of existing) {
    if (m.status) pending.push(m)
    else byId.set(m.id, m)
  }
  for (const m of incoming) {
    byId.set(m.id, m)
    if (m.referenceId) {
      const i = pending.findIndex((p) => p.referenceId === m.referenceId)
      if (i >= 0) pending.splice(i, 1)
    }
  }
  return [...[...byId.values()].sort((a, b) => a.id - b.id), ...pending]
}

/** Whether the shown messages reach the page, so merging them leaves no hole. */
function joins(shown: UiMessage[], page: ChatPage): boolean {
  const newest = shown.findLast((m) => !m.status)
  const oldest = page.oldestId ?? page.messages[0]?.id
  return !newest || oldest == null || newest.id >= oldest
}

function toUi(p: PendingMessage): UiMessage {
  return { ...p.message, status: p.state, error: p.error, progress: p.progress, localPath: p.localPath }
}

/** Replaces the pending bubbles of a chat by the outbox state. */
function withPending(messages: UiMessage[], pending: PendingMessage[]): UiMessage[] {
  const delivered = new Set(messages.filter((m) => !m.status && m.referenceId).map((m) => m.referenceId))
  // A delivered message can still be listed for a moment (until its echo is confirmed).
  return [...messages.filter((m) => !m.status), ...pending.filter((p) => !delivered.has(p.message.referenceId)).map(toUi)]
}

function updateOnly(existing: UiMessage[], incoming: MessageView[]): UiMessage[] {
  const updates = new Map(incoming.map((m) => [m.id, m]))
  return existing.map((m) => updates.get(m.id) ?? m)
}

// --- event handling ---------------------------------------------------------------

export function handleEvent(e: TalkEvent): void {
  switch (e.type) {
    case 'session':
      if (e.state === 'ready') set({ session: 'ready', account: e.account })
      else
        set({
          session: 'loggedOut',
          account: null,
          rooms: [],
          chats: {},
          activeToken: null,
          composing: false,
          groupInfo: null,
          sections: [],
          query: '',
          categories: false,
          unreadLine: null,
          settings: null,
        })
      break
    case 'roomsChanged':
      roomsChanged(e.rooms)
      break
    case 'participantsChanged':
      participantListeners.forEach((l) => l(e.token))
      break
    case 'messagesAdded':
      if (state.chats[e.token]) {
        countUnread(e.token, e.messages)
        setChat(e.token, (c) => ({ ...c, messages: merge(c.messages, e.messages) }))
      }
      break
    case 'messagesUpdated':
      if (state.chats[e.token]) setChat(e.token, (c) => ({ ...c, messages: updateOnly(c.messages, e.messages) }))
      break
    case 'readStatus':
      if (state.chats[e.token])
        setChat(e.token, (c) => ({ ...c, lastCommonRead: Math.max(c.lastCommonRead, e.lastCommonRead) }))
      break
    case 'connection':
      set({ online: e.online })
      break
    case 'openRoom':
      // A notification click shows the chat, also from the settings.
      set({ settings: null })
      void openRoom(e.token)
      break
    case 'openSettings':
      openSettings()
      break
    case 'outboxChanged':
      if (state.chats[e.token]) setChat(e.token, (c) => ({ ...c, messages: withPending(c.messages, e.items) }))
      break
    case 'authFailed':
      break
  }
}

/** Groups we leave or delete ourselves: no „nicht mehr in dieser Gruppe“ notice for them. */
const leaving = new Set<string>()

/**
 * New conversation list. When the open chat (or open group settings) disappears from it –
 * we were removed, the group was deleted, or we left – both close (docs/groups.md).
 */
function roomsChanged(rooms: RoomView[]): void {
  const gone = (token: string | null): token is string =>
    !!token && state.rooms.some((r) => r.token === token) && !rooms.some((r) => r.token === token)
  const patch: Partial<State> = { rooms }
  if (gone(state.groupInfo)) patch.groupInfo = null
  if (gone(state.activeToken)) {
    const token = state.activeToken
    void window.talk.closeChat(token)
    patch.activeToken = null
    patch.groupInfo = null
    patch.unreadLine = null
    if (!leaving.has(token)) toast('Du bist nicht mehr in dieser Gruppe')
  }
  for (const t of leaving) if (!rooms.some((r) => r.token === t)) leaving.delete(t)
  set(patch)
  updateBadge()
  void refreshSections()
}

let sectionsSeq = 0

/**
 * Asks the core for the sections again: on `roomsChanged`, a new search text and when
 * another chat opens. Only the newest answer counts.
 */
export async function refreshSections(): Promise<void> {
  const seq = ++sectionsSeq
  const sections = await window.talk.roomSections(state.query, state.activeToken).catch(() => null)
  if (sections && seq === sectionsSeq && state.session === 'ready') set({ sections })
}

export function setQuery(query: string): void {
  if (query === state.query) return
  set({ query })
  void refreshSections()
}

/** Opens or closes a section; the list follows via `roomsChanged`, an error comes as a toast. */
export async function toggleSection(id: string): Promise<void> {
  const error = await window.talk.toggleSection(id).catch(() => 'Keine Verbindung')
  if (error) toast(error)
}

/** The rows in the order shown, each chat once (Alt+↑/↓, Enter in the search). */
export function shownRooms(): RoomView[] {
  const seen = new Set<string>()
  const list: RoomView[] = []
  for (const section of state.sections)
    for (const r of section.rooms)
      if (!seen.has(r.token)) {
        seen.add(r.token)
        list.push(r)
      }
  return list
}

type ParticipantsListener = (token: string) => void
const participantListeners = new Set<ParticipantsListener>()

/** Called on `participantsChanged` (reload the member list). Returns the unsubscribe function. */
export function onParticipantsChanged(l: ParticipantsListener): () => void {
  participantListeners.add(l)
  return () => participantListeners.delete(l)
}

/** Leave or delete a group ourselves (no notice when it disappears). Resolves to an error text or null. */
export async function leaveOrDelete(token: string, del: boolean): Promise<string | null> {
  leaving.add(token)
  const error = await (del ? window.talk.deleteGroup(token) : window.talk.leaveGroup(token)).catch(
    () => 'Keine Verbindung',
  )
  if (error) leaving.delete(token)
  return error
}

export function openCompose(): void {
  set({ composing: true })
}
export function closeCompose(): void {
  set({ composing: false })
}
export function openGroupInfo(token: string): void {
  set({ groupInfo: token })
}
export function closeGroupInfo(): void {
  set({ groupInfo: null })
}
export function openCategories(): void {
  set({ categories: true, groupInfo: null })
}
export function closeCategories(): void {
  set({ categories: false })
}

export function openSettings(page: SettingsPage = 'appearance'): void {
  // Already open: stays on its page (⌘, from the menu and the key both arrive on macOS).
  if (state.session === 'ready') set({ settings: state.settings ?? page })
}
export function setSettingsPage(page: SettingsPage): void {
  set({ settings: page })
}
export function closeSettings(): void {
  set({ settings: null })
}

/**
 * Whether a chat counts for the badge (docs/settings.md, „Stummgeschaltete Chats“):
 * „Nur Erwähnungen“ only with a mention, „Aus“ never.
 */
export function countsForBadge(r: RoomView): boolean {
  if (r.isArchived || r.unreadMessages === 0) return false
  if (r.notificationLevel === 3) return false
  if (r.notificationLevel === 2) return r.unreadMention
  return true
}

function updateBadge(): void {
  window.talk.setBadge(state.rooms.filter(countsForBadge).length)
}

// --- actions ------------------------------------------------------------------------

export async function boot(): Promise<void> {
  window.talk.onEvent(handleEvent)
  // Waiting messages go out as soon as the network is back.
  window.addEventListener('online', () => window.talk.networkChanged())
  window.addEventListener('offline', () => window.talk.networkChanged())
  window.addEventListener('focus', refreshRoomsThrottled)
  const account = await window.talk.account()
  if (!account) {
    set({ session: 'loggedOut' })
    return
  }
  set({ session: 'ready', account, rooms: await window.talk.rooms() })
  updateBadge()
  void refreshSections()
}

export async function openRoom(token: string): Promise<void> {
  const previous = state.activeToken
  if (previous === token) {
    // Clicking the open chat in the list leaves its settings (or the page „Kategorien“).
    if (state.groupInfo || state.categories) set({ groupInfo: null, categories: false })
    return
  }
  if (previous) void window.talk.closeChat(previous)
  // The snapshot comes first: the chat marks itself read as soon as it shows the end.
  const opened = state.rooms.find((r) => r.token === token)
  // Always, also without unread messages: the list may not know the newest ones yet.
  const unreadLine =
    opened && opened.lastReadMessage > 0
      ? { token, from: opened.lastReadMessage, base: opened.unreadMessages }
      : null
  set({ activeToken: token, groupInfo: null, categories: false, unreadLine })
  void refreshSections()
  setChat(token, (c) => ({ ...c, loading: !c.messages.some((m) => !m.status), synced: false }))
  try {
    // Unsent messages survive restarts: show them from the first frame.
    const pending = await window.talk.pending(token)
    setChat(token, (c) => ({ ...c, messages: withPending(c.messages, pending) }))
    // Cached messages first, then the server state.
    const cached: ChatPage = await window.talk.cachedChat(token)
    if (state.activeToken === token && cached.messages.length > 0) {
      setChat(token, (c) => {
        const older = cached.oldestId != null && (c.oldestId == null || cached.oldestId < c.oldestId)
        return {
          ...c,
          messages: merge(c.messages, cached.messages),
          hasMore: older ? cached.hasMore : c.hasMore,
          oldestId: older ? cached.oldestId : c.oldestId,
          lastCommonRead: Math.max(c.lastCommonRead, cached.lastCommonRead ?? 0),
          loading: false,
        }
      })
    }
    const page: ChatPage = await window.talk.openChat(token)
    const room = state.rooms.find((r) => r.token === token)
    setChat(token, (c) => ({
      ...c,
      // Outdated cache, or more new messages than one page: drop what was shown, else a hole
      // would stay between it and the new page (unsent messages stay).
      messages: merge(page.reset || !joins(c.messages, page) ? c.messages.filter((m) => m.status) : c.messages, page.messages),
      hasMore: page.hasMore,
      oldestId: page.oldestId,
      lastCommonRead: Math.max(page.lastCommonRead ?? 0, room?.lastCommonReadMessage ?? 0),
      loading: false,
      synced: true,
    }))
  } catch {
    setChat(token, (c) => ({ ...c, loading: false, synced: true }))
  }
}

export function closeRoom(): void {
  if (state.activeToken) void window.talk.closeChat(state.activeToken)
  set({ activeToken: null, groupInfo: null, unreadLine: null })
  void refreshSections()
}

export async function loadOlder(token: string): Promise<void> {
  const chat = state.chats[token]
  if (!chat || !chat.hasMore || chat.loadingOlder || chat.oldestId == null) return
  setChat(token, (c) => ({ ...c, loadingOlder: true }))
  try {
    const page = await window.talk.loadOlder(token, chat.oldestId)
    setChat(token, (c) => ({
      ...c,
      messages: merge(c.messages, page.messages),
      hasMore: page.hasMore,
      oldestId: page.oldestId ?? c.oldestId,
      loadingOlder: false,
    }))
  } catch {
    setChat(token, (c) => ({ ...c, loadingOlder: false }))
  }
}

let lastRefresh = 0

/** Desktop counterpart of pull-to-refresh: re-sync the list when the window gets focus. */
function refreshRoomsThrottled(): void {
  if (state.session !== 'ready' || Date.now() - lastRefresh < 30_000) return
  lastRefresh = Date.now()
  void window.talk.refreshRooms()
}

export function toReply(m: UiMessage | null): ReplyPreview | null {
  return m ? { id: m.id, author: m.author, text: m.text, isOwn: m.isOwn, attachment: m.attachment ?? null } : null
}

/** Shows a queued message at once (the outbox event may arrive a moment later). */
export function showQueued(token: string, p: PendingMessage): void {
  // Every way of sending (text, files, voice) ends here: an own message ends the unread line.
  if (state.unreadLine?.token === token) set({ unreadLine: null })
  setChat(token, (c) =>
    c.messages.some((m) => m.status && m.referenceId === p.message.referenceId)
      ? c
      : { ...c, messages: [...c.messages, toUi(p)] },
  )
}

/** Queues the message in the outbox: it is delivered reliably, also after a restart. */
export async function sendMessage(token: string, text: string, replyTo: UiMessage | null): Promise<void> {
  showQueued(token, await window.talk.enqueueText(token, text, toReply(replyTo)))
}

/** Queues staged files; the caption goes with the last one (shown below the pictures). */
export async function sendFiles(
  token: string,
  files: StagedFile[],
  caption: string,
  replyTo: UiMessage | null,
): Promise<void> {
  for (const [i, f] of files.entries()) {
    const last = i === files.length - 1
    const p = await window.talk.enqueueFile(token, f, last ? caption : '', i === 0 ? toReply(replyTo) : null)
    showQueued(token, p)
  }
}

/** Sends a failed message again (or a waiting one right now). */
export function retry(msg: UiMessage): void {
  void window.talk.retrySend(msg.referenceId)
}

/** Drops a pending or failed message. */
export function discard(msg: UiMessage): void {
  void window.talk.discard(msg.referenceId)
}

/** Marks the newest message as read when the chat is actually visible. */
export function markVisibleRead(token: string): void {
  if (!document.hasFocus() || document.visibilityState !== 'visible') return
  const chat = state.chats[token]
  const room = state.rooms.find((r) => r.token === token)
  if (!chat || !room) return
  const newest = [...chat.messages].reverse().find((m) => !m.status)
  if (!newest) return
  if (room.unreadMessages > 0 || newest.id > room.lastReadMessage) {
    void window.talk.markRead(token, newest.id)
  }
}

/** Whether the open chat shows its end (the tight threshold of the read marker). */
let chatAtBottom = true

export function setChatAtBottom(atBottom: boolean): void {
  chatAtBottom = atBottom
}

/**
 * New messages in the open chat. A shown line counts them by itself. Without one (once the
 * opening is through), they start a new line when they arrive out of sight (scrolled up or
 * window in the background); in sight they are simply read.
 */
function countUnread(token: string, incoming: MessageView[]): void {
  if (state.activeToken !== token) return
  const chat = state.chats[token]
  const line = state.unreadLine?.token === token ? state.unreadLine : null
  // While opening, everything after the snapshot counts (the list may have been behind).
  if (!chat || (line && !chat.synced)) return
  if (line && chat.messages.some((m) => countsAsUnread(m, line.from))) return
  const newest = chat.messages.findLast((m) => !m.status)
  if (!newest || !incoming.some((m) => countsAsUnread(m, newest.id))) return
  const visible = chatAtBottom && document.hasFocus() && document.visibilityState === 'visible'
  set({ unreadLine: visible ? null : { token, from: newest.id, base: 0 } })
}
