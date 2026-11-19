// Renderer state: conversation list + loaded chats, fed by core events.
// A tiny external store (no framework) consumed via useSyncExternalStore.

import { useSyncExternalStore } from 'react'
import { toast } from './call/store'
import type {
  AccountInfo,
  AccountRow,
  AroundResult,
  ChatPage,
  ConnectionState,
  MentionPick,
  MessageView,
  PendingMessage,
  PendingState,
  ReplyPreview,
  RoomView,
  SearchHit,
  SearchResults,
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
  /**
   * The list is a window into the history (docs/search.md „Chat an einer Nachricht öffnen“):
   * newer messages follow below it, and nothing new is appended.
   */
  hasNewer: boolean
  /** For `loadNewer`: from the page loaded last in that direction, hidden messages count. */
  newestId: number | null
  loadingNewer: boolean
  /** New messages that arrived while the list is a window; they go to its end. */
  held: MessageView[]
  /** Counts the times the list was replaced: an answer for the list before is dropped. */
  view: number
  /**
   * Mentions of the user not seen yet, ascending (docs/mentions.md „„@“-Knopf im Chat“);
   * only while the chat is open.
   */
  mentions: number[]
  /** Seen since the chat opened: a later page does not bring them back. */
  mentionsSeen: number[]
}

/** Where the open chat should stand: at a message, or (`id` null) at its end. */
export interface Jump {
  token: string
  id: number | null
  seq: number
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
  /** The account shown (docs/accounts.md). */
  account: AccountInfo | null
  /** All signed-in accounts in the order of signing in, with their numbers. */
  accounts: AccountRow[]
  /** „Konto hinzufügen“: the login shows while the accounts go on. */
  adding: boolean
  rooms: RoomView[]
  chats: Record<string, ChatState>
  /** Of the account shown (docs/connection.md). */
  connection: ConnectionState
  /** `connection` has been other than 'online' for 500 ms: only then the header shows it. */
  connectionHeld: boolean
  activeToken: string | null
  /** „Neue Nachricht“ replaces the conversation list (docs/groups.md). */
  composing: boolean
  /** Token whose group settings replace the chat area. */
  groupInfo: string | null
  /** The chat list in sections (docs/categories.md), for `query` and the open chat. */
  sections: SectionView[]
  /** Search text of the chat list. */
  query: string
  /** The mark „in {Chat}“: the search is limited to this chat (docs/search.md). */
  scope: { token: string; name: string } | null
  /** Messages found for `query` and `scope`; null below two characters. */
  hits: SearchResults | null
  /** The search (`searchKey`) the hits belong to. */
  hitsFor: string
  /** The hit chosen in the list. */
  hit: { token: string; messageId: number } | null
  /** Counts the requests to put the focus into the search field. */
  searchFocus: number
  /** The newest request to place the open chat. */
  jump: Jump | null
  /** The page „Kategorien“ replaces the chat area. */
  categories: boolean
  unreadLine: UnreadLine | null
  /** Counts notification clicks on the chat that is open already: it places itself again. */
  reopened: number
  /** „Einstellungen“ replace the whole window (docs/settings.md); the page shown on the right. */
  settings: SettingsPage | null
}

export type SettingsPage = 'general' | 'appearance' | 'chats' | 'categories' | 'about'

let state: State = {
  session: 'loading',
  account: null,
  accounts: [],
  adding: false,
  rooms: [],
  chats: {},
  connection: 'connecting',
  connectionHeld: false,
  activeToken: null,
  composing: false,
  groupInfo: null,
  sections: [],
  query: '',
  scope: null,
  hits: null,
  hitsFor: '',
  hit: null,
  searchFocus: 0,
  jump: null,
  categories: false,
  unreadLine: null,
  reopened: 0,
  settings: null,
}
const listeners = new Set<() => void>()

/** What belongs to the account shown: emptied on a switch and when signing out. */
const EMPTY = {
  rooms: [],
  chats: {},
  activeToken: null,
  composing: false,
  groupInfo: null,
  sections: [],
  query: '',
  scope: null,
  hits: null,
  hitsFor: '',
  hit: null,
  jump: null,
  categories: false,
  unreadLine: null,
} satisfies Partial<State>

/** The account shown, for keys of drafts and caches: tokens of different servers overlap. */
export function accountId(): string {
  return state.account?.id ?? ''
}

function set(patch: Partial<State>): void {
  state = { ...state, ...patch }
  listeners.forEach((l) => l())
}

let connectionTimer = 0

/**
 * The connection state of the account shown, with `more` in the same step. A state other
 * than 'online' shows once it held for 500 ms; back online it is gone at once.
 */
function setConnection(connection: ConnectionState, more: Partial<State> = {}): void {
  const away = connection !== 'online'
  if (!away) {
    window.clearTimeout(connectionTimer)
    connectionTimer = 0
  } else if (!connectionTimer && !state.connectionHeld) {
    connectionTimer = window.setTimeout(() => {
      connectionTimer = 0
      set({ connectionHeld: true })
    }, 500)
  }
  set({ ...more, connection, connectionHeld: away && state.connectionHeld })
}

/** What the header shows: short phases without an answer stay invisible. */
export function shownConnection(s: State): ConnectionState {
  return s.connectionHeld ? s.connection : 'online'
}

function setChat(token: string, update: (c: ChatState) => ChatState): void {
  const current = state.chats[token] ?? emptyChat()
  set({ chats: { ...state.chats, [token]: update(current) } })
}

function emptyChat(): ChatState {
  return {
    messages: [],
    hasMore: true,
    oldestId: null,
    lastCommonRead: 0,
    loading: true,
    loadingOlder: false,
    synced: false,
    hasNewer: false,
    newestId: null,
    loadingNewer: false,
    held: [],
    view: 0,
    mentions: [],
    mentionsSeen: [],
  }
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

function updateOnly<T extends MessageView>(existing: T[], incoming: MessageView[]): T[] {
  const updates = new Map(incoming.map((m) => [m.id, m]))
  return existing.map((m) => (updates.get(m.id) as T | undefined) ?? m)
}

/** New messages of a window, kept for its end: each once, ascending. */
function hold(held: MessageView[], incoming: MessageView[]): MessageView[] {
  const byId = new Map(held.map((m) => [m.id, m]))
  for (const m of incoming) byId.set(m.id, m)
  return [...byId.values()].sort((a, b) => a.id - b.id)
}

/** The end of a window: the kept messages that are newer than the last one of the list. */
function appendHeld(messages: UiMessage[], held: MessageView[]): UiMessage[] {
  const last = messages.findLast((m) => !m.status)?.id ?? 0
  const newer = held.filter((m) => m.id > last)
  return newer.length > 0 ? merge(messages, newer) : messages
}

/** The chat with more mentions the user has not seen yet. */
function withMentions(c: ChatState, ids: number[] | undefined): ChatState {
  const fresh = (ids ?? []).filter((id) => !c.mentions.includes(id) && !c.mentionsSeen.includes(id))
  return fresh.length > 0 ? { ...c, mentions: [...c.mentions, ...fresh].sort((a, b) => a - b) } : c
}

/** The bubbles of these mentions were in sight (or are gone): the „@“ button drops them. */
export function mentionsSeen(token: string, ids: number[]): void {
  if (!state.chats[token]?.mentions.some((id) => ids.includes(id))) return
  setChat(token, (c) => ({
    ...c,
    mentions: c.mentions.filter((id) => !ids.includes(id)),
    mentionsSeen: [...c.mentionsSeen, ...ids],
  }))
}

/** The „@“ button: to the oldest mention not seen yet. */
export async function showMention(token: string): Promise<void> {
  const id = state.chats[token]?.mentions[0]
  if (id == null) return
  // The message is gone: the button would lead nowhere for good.
  if ((await showMessage(token, id)) === false) mentionsSeen(token, [id])
}

// --- event handling ---------------------------------------------------------------

export function handleEvent(e: TalkEvent): void {
  switch (e.type) {
    case 'session':
      if (e.state === 'loggedOut') {
        // The next sign-in starts as 'connecting' again, with its own 500 ms.
        window.clearTimeout(connectionTimer)
        connectionTimer = 0
        resetSearch()
        set({ ...EMPTY, session: 'loggedOut', account: null, accounts: [], adding: false, settings: null, connection: 'connecting', connectionHeld: false })
      } else if (state.account?.id === e.account.id) setConnection(e.connection, { session: 'ready', account: e.account })
      else {
        // Another account (docs/accounts.md „Wechseln“): its list at once, nothing of the
        // one before. Open settings stay open; drafts stay with their account.
        resetSearch()
        setConnection(e.connection, { ...EMPTY, session: 'ready', account: e.account, rooms: e.rooms ?? [], adding: false })
        void refreshSections()
        if (!e.rooms) void window.talk.rooms().then((rooms) => set({ rooms }))
      }
      break
    case 'accountsChanged':
      set({ accounts: e.accounts })
      break
    case 'notice':
      toast(e.text)
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
        // In a window they would leave a hole: kept and counted until its end is reached.
        // Only the open chat keeps the list for its „@“ button.
        const mentioning = state.activeToken === e.token ? e.messages.filter((m) => m.mentionsMe).map((m) => m.id) : []
        setChat(e.token, (c) =>
          withMentions(
            c.hasNewer ? { ...c, held: hold(c.held, e.messages) } : { ...c, messages: merge(c.messages, e.messages) },
            mentioning,
          ),
        )
      }
      refreshHits()
      break
    case 'messagesUpdated':
      if (state.chats[e.token]) {
        const deleted = e.messages.filter((m) => m.kind === 'deleted').map((m) => m.id)
        setChat(e.token, (c) => ({
          ...c,
          messages: updateOnly(c.messages, e.messages),
          held: updateOnly(c.held, e.messages),
          // A deleted message mentions nobody any more.
          mentions: deleted.length > 0 ? c.mentions.filter((id) => !deleted.includes(id)) : c.mentions,
        }))
      }
      refreshHits()
      break
    case 'readStatus':
      if (state.chats[e.token])
        setChat(e.token, (c) => ({ ...c, lastCommonRead: Math.max(c.lastCommonRead, e.lastCommonRead) }))
      break
    case 'connection':
      setConnection(e.state)
      break
    case 'openRoom':
      // A notification click shows the chat, also from the settings.
      set({ settings: null })
      // The chat is open already (docs/notifications.md „War der Chat schon offen“): the
      // view stands as after a new opening, at the unread line or else at the end.
      if (state.activeToken === e.token) {
        const again = () => set({ unreadLine: lineOnOpening(e.token), reopened: state.reopened + 1 })
        // Out of a window first: the line and the end are in the newest messages.
        if (state.chats[e.token]?.hasNewer) void showEnd(e.token, false).then(again)
        else again()
      }
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
  // The chat of the mark „in {Chat}“ is gone: the text searches all chats again.
  const unscoped = gone(state.scope?.token ?? null)
  if (unscoped) patch.scope = null
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
  void refreshSections()
  // New messages of chats that are not open reach the index with the chat list.
  if (unscoped) newSearch()
  else refreshHits()
}

let sectionsSeq = 0

/**
 * Asks the core for the sections again: on `roomsChanged`, a new search text and when
 * another chat opens. Only the newest answer counts.
 */
export async function refreshSections(): Promise<void> {
  const seq = ++sectionsSeq
  // With the mark „in {Chat}“ the text searches that chat, not the names: the whole list
  // stays behind the hits (Alt+↑/↓).
  const sections = await window.talk.roomSections(state.scope ? '' : state.query, state.activeToken).catch(() => null)
  if (sections && seq === sectionsSeq && state.session === 'ready') set({ sections })
}

export function setQuery(query: string): void {
  if (query === state.query) return
  set({ query, hit: null })
  void refreshSections()
  newSearch()
}

// --- search in the messages (docs/search.md) ------------------------------------------------

/** The list starts with this many hits and grows by as many. */
const HITS_PAGE = 50
/** In one chat: the row of hits the list shows. */
const HITS_IN_CHAT = 500
/** The server's hits are asked for this long after the last keystroke. */
const EVERYWHERE_MS = 300
/** While the history is still loading, the search runs again this often. */
const LOADING_TICK_MS = 1000

let hitLimit = HITS_PAGE
let growing = false
let searchSeq = 0
let everywhereTimer = 0
let tickTimer = 0
/** The search whose shown hits the server completed. */
let supplemented: string | null = null

/** What a search is: the chat it is limited to and the text. */
export function searchKey(s: State = state): string {
  return `${s.scope?.token ?? ''}\n${s.query}`
}

/** From two characters on, spaces not counted. */
export function searchable(query: string): boolean {
  return query.replace(/\s/g, '').length >= 2
}

/** Another account: nothing of the search before arrives any more. */
function resetSearch(): void {
  searchSeq += 1
  window.clearTimeout(everywhereTimer)
  window.clearTimeout(tickTimer)
  supplemented = null
  hitLimit = HITS_PAGE
  growing = false
}

/** Another search text or another chat: from the start. */
function newSearch(): void {
  hitLimit = HITS_PAGE
  growing = false
  void runSearch('typed')
}

function refreshHits(): void {
  if (state.session === 'ready' && searchable(state.query)) void runSearch('refresh')
}

/**
 * Searches on the device, on every change of the search text without waiting. While the
 * history is not complete there, the server's hits follow 300 ms after the last keystroke.
 * `tick` only watches the loading history.
 */
async function runSearch(reason: 'typed' | 'refresh' | 'tick'): Promise<void> {
  const seq = ++searchSeq
  window.clearTimeout(tickTimer)
  const { query } = state
  const token = state.scope?.token ?? null
  const key = searchKey()
  if (!searchable(query)) {
    window.clearTimeout(everywhereTimer)
    supplemented = null
    if (state.hits || state.hitsFor !== key) set({ hits: null, hitsFor: key })
    return
  }
  const limit = token ? HITS_IN_CHAT : hitLimit
  const found = await window.talk.searchMessages(query, token, limit).catch(() => null)
  // A result for an outdated search text is dropped.
  if (!found || seq !== searchSeq || state.session !== 'ready') return
  if (found.complete) supplemented = null
  if (!found.complete && supplemented === key && state.hits) {
    // The list with the server's hits stays until its successor is there.
    set({ hits: { ...state.hits, chatsDone: found.chatsDone, chatsTotal: found.chatsTotal } })
  } else {
    growing = false
    set({ hits: found, hitsFor: key })
  }
  if (!found.complete && reason !== 'tick') {
    window.clearTimeout(everywhereTimer)
    everywhereTimer = window.setTimeout(() => {
      window.talk.searchMessagesEverywhere(query, token, limit).then(
        (all) => {
          if (searchKey() !== key || state.session !== 'ready') return
          supplemented = key
          growing = false
          set({ hits: all, hitsFor: key })
        },
        // No network: the list stays as it is, without a notice.
        () => {
          growing = false
        },
      )
    }, EVERYWHERE_MS)
  }
  if (found.chatsDone < found.chatsTotal) tickTimer = window.setTimeout(() => void runSearch('tick'), LOADING_TICK_MS)
}

/** The end of the hit list is reached: 50 more, when there are more. */
export function moreHits(): void {
  if (growing || state.scope || !state.hits?.more || state.hitsFor !== searchKey()) return
  growing = true
  hitLimit += HITS_PAGE
  void runSearch('typed')
}

/** Sets or removes the mark „in {Chat}“; the search text stays. */
export function setScope(room: { token: string; name: string } | null): void {
  if ((room?.token ?? null) === (state.scope?.token ?? null)) return
  set({ scope: room ? { token: room.token, name: room.name } : null, hit: null })
  void refreshSections()
  newSearch()
}

/** Esc in the search field: text and mark are gone. */
export function clearSearch(): void {
  if (!state.query && !state.scope) return
  set({ query: '', scope: null, hit: null })
  void refreshSections()
  newSearch()
}

/**
 * The focus goes into the search field: for the open chat (magnifier, Ctrl+Shift+F) or
 * without a mark for all chats (Ctrl+F).
 */
export function focusSearch(inChat: boolean): void {
  const room = inChat ? state.rooms.find((r) => r.token === state.activeToken) : null
  setScope(room ?? null)
  set({ searchFocus: state.searchFocus + 1 })
}

/** A click on a hit: it stays marked in the list, the chat shows the message. */
export function openHit(hit: Pick<SearchHit, 'token' | 'messageId'>): void {
  set({ hit: { token: hit.token, messageId: hit.messageId } })
  void showMessage(hit.token, hit.messageId)
}

/** Arrow keys in the search field: the next older (1) or newer (-1) hit of the list. */
export function moveHit(delta: number): void {
  const list = state.hits?.hits ?? []
  const i = list.findIndex((h) => h.token === state.hit?.token && h.messageId === state.hit?.messageId)
  const next = i < 0 ? (delta > 0 ? list[0] : undefined) : list[Math.max(0, Math.min(list.length - 1, i + delta))]
  if (next) set({ hit: { token: next.token, messageId: next.messageId } })
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

// --- accounts (docs/accounts.md) -------------------------------------------------------

/** Shows another account; refused during a call (a toast says so). */
export async function switchAccount(id: string): Promise<void> {
  if (id === state.account?.id) return
  const refused = await window.talk.switchAccount(id).catch(() => 'Wechsel nicht möglich')
  if (refused) toast(refused)
}

/** „Konto hinzufügen“: the login, with „Abbrechen“ back to the chats. */
export function startAddAccount(): void {
  if (state.accounts.length < MAX_ACCOUNTS) set({ adding: true })
}
export function cancelAddAccount(): void {
  void window.talk.cancelLogin()
  set({ adding: false })
}
export const MAX_ACCOUNTS = 5

// --- actions ------------------------------------------------------------------------

export async function boot(): Promise<void> {
  window.talk.onEvent(handleEvent)
  // Waiting messages go out as soon as the network is back.
  window.addEventListener('online', () => window.talk.networkChanged())
  // No network at all (docs/connection.md): shows at once instead of after a call that
  // hangs. A call and the outbox get their probe as before.
  window.addEventListener('offline', () => {
    window.talk.networkLost()
    window.talk.networkChanged()
  })
  const [shown, accounts] = await Promise.all([window.talk.account(), window.talk.accounts()])
  if (state.accounts.length === 0) set({ accounts: accounts.accounts })
  // An event may have been quicker (a switch, the start finishing).
  if (state.session !== 'loading') return
  if (!shown) {
    set({ session: 'loggedOut' })
    return
  }
  setConnection(shown.connection, { session: 'ready', account: shown.account, rooms: shown.rooms })
  void refreshSections()
}

/** The unread line of a chat that opens now, from what the chat list knows about it. */
function lineOnOpening(token: string): UnreadLine | null {
  const room = state.rooms.find((r) => r.token === token)
  // Always, also without unread messages: the list may not know the newest ones yet.
  return room && room.lastReadMessage > 0 ? { token, from: room.lastReadMessage, base: room.unreadMessages } : null
}

/** A chat left as a window starts anew the next time: its list has no end. */
function withoutWindow(token: string | null): Record<string, ChatState> {
  if (!token || !state.chats[token]?.hasNewer) return state.chats
  const { [token]: _gone, ...rest } = state.chats
  return rest
}

/** Opens a chat; with `at` it shows that message instead of its end (docs/search.md). */
export async function openRoom(token: string, at?: number): Promise<void> {
  const previous = state.activeToken
  if (previous === token) {
    // Clicking the open chat in the list leaves its settings (or the page „Kategorien“).
    if (state.groupInfo || state.categories) set({ groupInfo: null, categories: false })
    if (at != null) void showMessage(token, at)
    return
  }
  if (at != null) return openAt(token, at)
  if (previous) void window.talk.closeChat(previous)
  // The snapshot comes first: the chat marks itself read as soon as it shows the end.
  set({
    activeToken: token,
    groupInfo: null,
    categories: false,
    unreadLine: lineOnOpening(token),
    chats: withoutWindow(previous),
    jump: null,
  })
  void refreshSections()
  // The list of unseen mentions starts anew with every opening.
  setChat(token, (c) => ({ ...c, loading: !c.messages.some((m) => !m.status), synced: false, mentions: [], mentionsSeen: [] }))
  await loadEnd(token)
}

/** The end of a chat that opens: unsent messages, the cache, then the server's page. */
async function loadEnd(token: string): Promise<void> {
  try {
    // Unsent messages survive restarts: show them from the first frame.
    const pending = await window.talk.pending(token)
    setChat(token, (c) => ({ ...c, messages: withPending(c.messages, pending) }))
    // Cached messages first, then the server state.
    const cached: ChatPage = await window.talk.cachedChat(token)
    if (state.activeToken === token && cached.messages.length > 0) {
      setChat(token, (c) => {
        // A jump was quicker: the window stays.
        if (c.hasNewer) return withMentions(c, cached.unreadMentions)
        // The cache went on while the chat was closed (the history loads in the background):
        // what an earlier visit left here does not reach it, and merging would leave a hole.
        const apart = !joins(c.messages, cached)
        const older = apart || (cached.oldestId != null && (c.oldestId == null || cached.oldestId < c.oldestId))
        return withMentions(
          {
            ...c,
            messages: merge(apart ? c.messages.filter((m) => m.status) : c.messages, cached.messages),
            hasMore: older ? cached.hasMore : c.hasMore,
            oldestId: older ? cached.oldestId : c.oldestId,
            lastCommonRead: Math.max(c.lastCommonRead, cached.lastCommonRead ?? 0),
            loading: false,
          },
          cached.unreadMentions,
        )
      })
    }
    const page: ChatPage = await window.talk.openChat(token)
    const room = state.rooms.find((r) => r.token === token)
    const lastCommonRead = Math.max(page.lastCommonRead ?? 0, room?.lastCommonReadMessage ?? 0)
    // Left meanwhile: the mentions belong to the chat that is open.
    const mentions = state.activeToken === token ? page.unreadMentions : []
    setChat(token, (c) =>
      withMentions(
        c.hasNewer
          ? { ...c, lastCommonRead, loading: false, synced: true }
          : {
              ...c,
              // Outdated cache, or more new messages than one page: drop what was shown, else a hole
              // would stay between it and the new page (unsent messages stay).
              messages: merge(page.reset || !joins(c.messages, page) ? c.messages.filter((m) => m.status) : c.messages, page.messages),
              hasMore: page.hasMore,
              oldestId: page.oldestId,
              lastCommonRead,
              loading: false,
              synced: true,
            },
        mentions,
      ),
    )
  } catch {
    setChat(token, (c) => ({ ...c, loading: false, synced: true }))
  }
}

let jumpSeq = 0
/** Counts the requests to show a message or the end: only the newest answer counts. */
let jumpAsk = 0

/**
 * Another chat opens at a message (a hit of the search over all chats). The window comes
 * first: without it the chat shown stays as it is. Then the chat opens with `openChat`,
 * which starts the live messages as always, but its page is not shown: the end of the chat
 * does not flash up.
 */
async function openAt(token: string, id: number): Promise<void> {
  const previous = state.activeToken
  const ask = ++jumpAsk
  const [around, pending] = await Promise.all([
    window.talk.messagesAround(token, id).catch((): AroundResult => ({ error: 'Keine Verbindung' })),
    window.talk.pending(token).catch(() => [] as PendingMessage[]),
  ])
  // Another hit or another chat was quicker.
  if (ask !== jumpAsk || state.activeToken !== previous) return
  if ('error' in around || !around.messages.some((m) => m.id === id)) {
    toast('error' in around ? around.error : 'Nachricht nicht gefunden')
    return
  }
  if (previous) void window.talk.closeChat(previous)
  const opening: Promise<ChatPage> = window.talk.openChat(token)
  const chats = withoutWindow(previous)
  // Nothing of an earlier visit: the list is the window.
  const chat = windowed({ ...emptyChat(), view: chats[token]?.view ?? 0 }, around, pending)
  set({
    activeToken: token,
    groupInfo: null,
    categories: false,
    // The read marker stays where it is; the line shows once its place is loaded.
    unreadLine: lineOnOpening(token),
    chats: { ...chats, [token]: chat },
    jump: { token, id, seq: ++jumpSeq },
  })
  void refreshSections()
  const room = state.rooms.find((r) => r.token === token)
  try {
    const page = await opening
    setChat(token, (c) => {
      const base = {
        ...withMentions(c, state.activeToken === token ? page.unreadMentions : []),
        lastCommonRead: Math.max(c.lastCommonRead, page.lastCommonRead ?? 0, room?.lastCommonReadMessage ?? 0),
        synced: true,
      }
      if (c.hasNewer) return base
      // The message is among the newest and the list reached the end already. More new
      // messages than one page: the list stays a window and goes on after its last message.
      if (page.reset || !joins(c.messages, page))
        return { ...base, hasNewer: true, newestId: c.messages.findLast((m) => !m.status)?.id ?? null }
      return { ...base, messages: merge(c.messages, page.messages) }
    })
  } catch {
    setChat(token, (c) => ({ ...c, synced: true }))
  }
}

/** The chat `c` with its list replaced by a window. */
function windowed(c: ChatState, page: ChatPage, pending?: PendingMessage[]): ChatState {
  let messages = merge(c.messages.filter((m) => m.status), page.messages)
  if (pending) messages = withPending(messages, pending)
  // Entering from the end, nothing is kept yet.
  let held = c.hasNewer ? c.held : []
  if (!page.hasNewer) {
    messages = appendHeld(messages, held)
    held = []
  }
  return {
    ...c,
    messages,
    hasMore: page.hasMore,
    oldestId: page.oldestId,
    lastCommonRead: Math.max(c.lastCommonRead, page.lastCommonRead ?? 0),
    loading: false,
    loadingOlder: false,
    loadingNewer: false,
    hasNewer: page.hasNewer,
    newestId: page.newestId,
    held,
    view: c.view + 1,
  }
}

/**
 * Shows a message (a search hit, a quoted message; docs/search.md „Chat an einer Nachricht
 * öffnen“): scrolls to it when it is loaded, else the list becomes a window around it.
 * True when it shows, false when there is no such message; null when nothing can be said
 * (no connection, another request was quicker).
 */
export async function showMessage(token: string, id: number): Promise<boolean | null> {
  if (state.activeToken !== token) {
    await openRoom(token, id)
    return null
  }
  if (state.groupInfo || state.categories) set({ groupInfo: null, categories: false })
  const ask = ++jumpAsk
  if (state.chats[token]?.messages.some((m) => !m.status && m.id === id)) {
    set({ jump: { token, id, seq: ++jumpSeq } })
    return true
  }
  const around = await window.talk.messagesAround(token, id).catch(() => ({ error: 'Keine Verbindung' }))
  if (state.activeToken !== token || ask !== jumpAsk) return null
  // The chat stays as it is.
  if ('error' in around) {
    toast(around.error)
    // Without a connection the message may well be there.
    return around.error === 'Nachricht nicht gefunden' ? false : null
  }
  if (!around.messages.some((m) => m.id === id)) {
    toast('Nachricht nicht gefunden')
    return false
  }
  set({
    chats: { ...state.chats, [token]: windowed(state.chats[token] ?? emptyChat(), around) },
    jump: { token, id, seq: ++jumpSeq },
  })
  return true
}

/**
 * „Zum Ende springen“ in a window: the newest messages replace the list, and the chat goes
 * to its end (`place` false: the caller places it).
 */
export async function showEnd(token: string, place = true): Promise<void> {
  if (!state.chats[token]?.hasNewer) return
  const ask = ++jumpAsk
  const cached = await window.talk.cachedChat(token).catch(() => null)
  const c = state.chats[token]
  if (state.activeToken !== token || ask !== jumpAsk || !c?.hasNewer) return
  if (!cached || cached.messages.length === 0) {
    toast('Keine Verbindung')
    return
  }
  const chat: ChatState = {
    ...c,
    messages: appendHeld(merge(c.messages.filter((m) => m.status), cached.messages), c.held),
    hasMore: cached.hasMore,
    oldestId: cached.oldestId,
    lastCommonRead: Math.max(c.lastCommonRead, cached.lastCommonRead ?? 0),
    loadingOlder: false,
    loadingNewer: false,
    hasNewer: false,
    newestId: null,
    held: [],
    view: c.view + 1,
  }
  set({ chats: { ...state.chats, [token]: chat }, jump: place ? { token, id: null, seq: ++jumpSeq } : null })
}

export function closeRoom(): void {
  const token = state.activeToken
  if (token) void window.talk.closeChat(token)
  set({ activeToken: null, groupInfo: null, unreadLine: null, chats: withoutWindow(token), jump: null })
  void refreshSections()
}

export async function loadOlder(token: string): Promise<void> {
  const chat = state.chats[token]
  if (!chat || !chat.hasMore || chat.loadingOlder || chat.oldestId == null) return
  const { view } = chat
  setChat(token, (c) => ({ ...c, loadingOlder: true }))
  try {
    const page = await window.talk.loadOlder(token, chat.oldestId)
    // The list was replaced meanwhile (a jump): the page belongs to the one before.
    if (state.chats[token]?.view !== view) return
    setChat(token, (c) => ({
      ...c,
      messages: merge(c.messages, page.messages),
      hasMore: page.hasMore,
      oldestId: page.oldestId ?? c.oldestId,
      loadingOlder: false,
    }))
  } catch {
    if (state.chats[token]?.view === view) setChat(token, (c) => ({ ...c, loadingOlder: false }))
  }
}

type AppendListener = (token: string) => void
let appendListener: AppendListener | null = null

/** Called right before a window grows downwards, so the chat can keep its place. */
export function onBeforeAppend(l: AppendListener): () => void {
  appendListener = l
  return () => {
    if (appendListener === l) appendListener = null
  }
}

/** The next messages below a window. At its end the kept messages follow, and it is none any more. */
export async function loadNewer(token: string): Promise<void> {
  const chat = state.chats[token]
  if (!chat || !chat.hasNewer || chat.loadingNewer || chat.newestId == null) return
  const { view, newestId } = chat
  setChat(token, (c) => ({ ...c, loadingNewer: true }))
  try {
    const page = await window.talk.loadNewer(token, newestId)
    if (state.chats[token]?.view !== view) return
    appendListener?.(token)
    setChat(token, (c) => {
      const messages = merge(c.messages, page.messages)
      return {
        ...c,
        // Only what is newer than the last message of the list: nothing twice, no hole.
        messages: page.hasNewer ? messages : appendHeld(messages, c.held),
        hasNewer: page.hasNewer,
        newestId: page.newestId ?? c.newestId,
        held: page.hasNewer ? c.held : [],
        loadingNewer: false,
      }
    })
    // A page of hidden messages only shows nothing: the next one right away.
    if (page.hasNewer && page.messages.length === 0 && page.newestId != null && page.newestId > newestId) void loadNewer(token)
  } catch {
    if (state.chats[token]?.view === view) setChat(token, (c) => ({ ...c, loadingNewer: false }))
  }
}

export function toReply(m: UiMessage | null): ReplyPreview | null {
  return m
    ? { id: m.id, author: m.author, text: m.text, isOwn: m.isOwn, attachment: m.attachment ?? null, location: m.location ?? null }
    : null
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
  // Unsent messages stand at the end of the chat: sending in a window leads there.
  if (state.chats[token]?.hasNewer) void showEnd(token)
}

/** Queues the message in the outbox: it is delivered reliably, also after a restart. */
export async function sendMessage(
  token: string,
  text: string,
  mentions: MentionPick[],
  replyTo: UiMessage | null,
): Promise<void> {
  showQueued(token, await window.talk.enqueueText(token, text, mentions, toReply(replyTo)))
}

/** Queues staged files; the caption and its mentions go with the last one (shown below the pictures). */
export async function sendFiles(
  token: string,
  files: StagedFile[],
  caption: string,
  mentions: MentionPick[],
  replyTo: UiMessage | null,
): Promise<void> {
  for (const [i, f] of files.entries()) {
    const last = i === files.length - 1
    const p = await window.talk.enqueueFile(token, f, last ? caption : '', last ? mentions : [], i === 0 ? toReply(replyTo) : null)
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
  // In a window the read marker does not move: its last message is not the newest.
  if (!chat || !room || chat.hasNewer) return
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
  if (chat.hasNewer) {
    // In a window everything new is out of sight. A line that still counts stays; else a
    // new one starts at the first foreign message (it shows once the end is loaded).
    const first = incoming.find((m) => !m.isOwn && m.kind !== 'system')
    if (!first) return
    if (line && (line.base > 0 || [...chat.messages, ...chat.held].some((m) => countsAsUnread(m, line.from)))) return
    set({ unreadLine: { token, from: first.id - 1, base: 0 } })
    return
  }
  if (line && chat.messages.some((m) => countsAsUnread(m, line.from))) return
  const newest = chat.messages.findLast((m) => !m.status)
  if (!newest || !incoming.some((m) => countsAsUnread(m, newest.id))) return
  const visible = chatAtBottom && document.hasFocus() && document.visibilityState === 'visible'
  set({ unreadLine: visible ? null : { token, from: newest.id, base: 0 } })
}
