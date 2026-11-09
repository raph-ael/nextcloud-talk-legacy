// Renderer state: conversation list + loaded chats, fed by core events.
// A tiny external store (no framework) consumed via useSyncExternalStore.

import { useSyncExternalStore } from 'react'
import type {
  AccountInfo,
  ChatPage,
  MessageView,
  PendingMessage,
  PendingState,
  ReplyPreview,
  RoomView,
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
}

export interface State {
  session: 'loading' | 'loggedOut' | 'ready'
  account: AccountInfo | null
  rooms: RoomView[]
  chats: Record<string, ChatState>
  online: boolean
  activeToken: string | null
}

let state: State = {
  session: 'loading',
  account: null,
  rooms: [],
  chats: {},
  online: true,
  activeToken: null,
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
  return { messages: [], hasMore: true, oldestId: null, lastCommonRead: 0, loading: true, loadingOlder: false }
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
      else set({ session: 'loggedOut', account: null, rooms: [], chats: {}, activeToken: null })
      break
    case 'roomsChanged':
      set({ rooms: e.rooms })
      updateBadge()
      break
    case 'messagesAdded':
      if (state.chats[e.token]) setChat(e.token, (c) => ({ ...c, messages: merge(c.messages, e.messages) }))
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
      void openRoom(e.token)
      break
    case 'outboxChanged':
      if (state.chats[e.token]) setChat(e.token, (c) => ({ ...c, messages: withPending(c.messages, e.items) }))
      break
    case 'authFailed':
      break
  }
}

function updateBadge(): void {
  const total = state.rooms.filter((r) => !r.isArchived && r.unreadMessages > 0).length
  window.talk.setBadge(total)
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
}

export async function openRoom(token: string): Promise<void> {
  const previous = state.activeToken
  if (previous === token) return
  if (previous) void window.talk.closeChat(previous)
  set({ activeToken: token })
  setChat(token, (c) => ({ ...c, loading: !c.messages.some((m) => !m.status) }))
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
      // Outdated cache: drop what was shown (unsent messages stay).
      messages: merge(page.reset ? c.messages.filter((m) => m.status) : c.messages, page.messages),
      hasMore: page.hasMore,
      oldestId: page.oldestId,
      lastCommonRead: Math.max(page.lastCommonRead ?? 0, room?.lastCommonReadMessage ?? 0),
      loading: false,
    }))
  } catch {
    setChat(token, (c) => ({ ...c, loading: false }))
  }
}

export function closeRoom(): void {
  if (state.activeToken) void window.talk.closeChat(state.activeToken)
  set({ activeToken: null })
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

function toReply(m: UiMessage | null): ReplyPreview | null {
  return m ? { id: m.id, author: m.author, text: m.text, isOwn: m.isOwn, attachment: m.attachment ?? null } : null
}

/** Shows a queued message at once (the outbox event may arrive a moment later). */
function showQueued(token: string, p: PendingMessage): void {
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
