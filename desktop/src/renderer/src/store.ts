// Renderer state: conversation list + loaded chats, fed by core events.
// A tiny external store (no framework) consumed via useSyncExternalStore.

import { useSyncExternalStore } from 'react'
import type { AccountInfo, ChatPage, MessageView, RoomView, TalkEvent } from '@shared/core'

export type SendStatus = 'sending' | 'failed'
export type UiMessage = MessageView & { status?: SendStatus }

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
  setChat(token, (c) => ({ ...c, loading: c.messages.length === 0 }))
  try {
    const page: ChatPage = await window.talk.openChat(token)
    const room = state.rooms.find((r) => r.token === token)
    setChat(token, (c) => ({
      ...c,
      messages: merge(c.messages, page.messages),
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

function referenceId(): string {
  const bytes = crypto.getRandomValues(new Uint8Array(32))
  return Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('')
}

export async function sendMessage(token: string, text: string, replyTo: UiMessage | null): Promise<void> {
  const ref = referenceId()
  const account = state.account
  const optimistic: UiMessage = {
    id: -Date.now(),
    token,
    actorType: 'users',
    actorId: account?.userId ?? '',
    author: '',
    timestamp: Math.floor(Date.now() / 1000),
    text,
    kind: 'text',
    systemMessage: '',
    referenceId: ref,
    isOwn: true,
    edited: false,
    markdown: true,
    replyTo: replyTo ? { id: replyTo.id, author: replyTo.author, text: replyTo.text, isOwn: replyTo.isOwn } : null,
    reactions: {},
    reactionsSelf: [],
    status: 'sending',
  }
  setChat(token, (c) => ({ ...c, messages: [...c.messages, optimistic] }))
  await deliver(token, optimistic)
}

async function deliver(token: string, msg: UiMessage): Promise<void> {
  try {
    const sent = await window.talk.send(token, msg.text, msg.referenceId, msg.replyTo?.id ?? null)
    setChat(token, (c) => ({ ...c, messages: merge(c.messages, [sent]) }))
  } catch {
    setChat(token, (c) => ({
      ...c,
      messages: c.messages.map((m) => (m.referenceId === msg.referenceId && m.status ? { ...m, status: 'failed' } : m)),
    }))
  }
}

export function retry(token: string, msg: UiMessage): void {
  setChat(token, (c) => ({
    ...c,
    messages: c.messages.map((m) => (m.referenceId === msg.referenceId ? { ...m, status: 'sending' } : m)),
  }))
  void deliver(token, msg)
}

export function discard(token: string, msg: UiMessage): void {
  setChat(token, (c) => ({ ...c, messages: c.messages.filter((m) => m.referenceId !== msg.referenceId || !m.status) }))
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
