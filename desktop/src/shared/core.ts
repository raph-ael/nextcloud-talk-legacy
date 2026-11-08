// Types of the data produced by the Rust core (talk-core/src/engine.rs, serde camelCase).
// Keep in sync with the Rust structs.

/** A file shared into a conversation; width/height are 0 when unknown. */
export interface FileAttachment {
  id: string
  name: string
  path: string
  mimetype: string
  size: number
  width: number
  height: number
  previewAvailable: boolean
  link: string
  voiceMessage: boolean
}

/** Short label for a file message ("📷 Foto", "📎 name.pdf"), followed by its caption. */
export function withAttachment(text: string, a: FileAttachment | null | undefined): string {
  if (!a) return text
  const label = a.voiceMessage
    ? '🎤 Sprachnachricht'
    : a.mimetype.startsWith('image/')
      ? '📷 Foto'
      : a.mimetype.startsWith('video/')
        ? '🎬 Video'
        : `📎 ${a.name}`
  return text ? `${label} · ${text}` : label
}

export interface MessagePreview {
  author: string
  /** For file messages only the caption (may be empty). */
  text: string
  isOwn: boolean
  isSystem: boolean
  attachment: FileAttachment | null
}

export interface RoomView {
  token: string
  name: string
  description: string
  /** 1 one-to-one, 2 group, 3 public, 4 changelog, 5 former 1:1, 6 note to self */
  roomType: number
  unreadMessages: number
  unreadMention: boolean
  lastActivity: number
  lastReadMessage: number
  lastCommonReadMessage: number
  lastMessage: MessagePreview | null
  isFavorite: boolean
  isArchived: boolean
  readOnly: boolean
  hasCall: boolean
  /** 0 default, 1 always, 2 mentions only, 3 never */
  notificationLevel: number
  avatarVersion: string
}

export interface ReplyPreview {
  id: number
  author: string
  text: string
  isOwn: boolean
  attachment?: FileAttachment | null
}

export interface MessageView {
  id: number
  token: string
  actorType: string
  actorId: string
  author: string
  timestamp: number
  text: string
  kind: 'text' | 'system' | 'deleted'
  systemMessage: string
  referenceId: string
  isOwn: boolean
  edited: boolean
  /** Render `text` as Markdown (Talk's default for chat messages). */
  markdown: boolean
  replyTo: ReplyPreview | null
  reactions: Record<string, number>
  reactionsSelf: string[]
  /** For file messages; `text` is then the caption. */
  attachment?: FileAttachment | null
}

export interface ChatPage {
  messages: MessageView[]
  hasMore: boolean
  oldestId: number | null
  lastCommonRead: number | null
}

export type CoreEvent =
  | { type: 'roomsChanged'; rooms: RoomView[] }
  | { type: 'messagesAdded'; token: string; messages: MessageView[] }
  | { type: 'messagesUpdated'; token: string; messages: MessageView[] }
  | { type: 'readStatus'; token: string; lastCommonRead: number }
  | { type: 'connection'; online: boolean }
  | { type: 'authFailed' }

/** Events produced by the Electron main process itself. */
export type AppEvent =
  | { type: 'session'; state: 'loggedOut' }
  | { type: 'session'; state: 'ready'; account: AccountInfo }
  | { type: 'openRoom'; token: string }

export type TalkEvent = CoreEvent | AppEvent

export interface AccountInfo {
  server: string
  user: string
  userId: string
}

/** API exposed by the preload script as `window.talk`. */
export interface TalkApi {
  account(): Promise<AccountInfo | null>
  /** Opens the browser and resolves once the login was approved (or rejects). */
  login(server: string): Promise<void>
  cancelLogin(): Promise<void>
  logout(): Promise<void>
  rooms(): Promise<RoomView[]>
  openChat(token: string): Promise<ChatPage>
  closeChat(token: string): Promise<void>
  loadOlder(token: string, beforeId: number): Promise<ChatPage>
  send(token: string, text: string, referenceId: string, replyTo: number | null): Promise<MessageView>
  markRead(token: string, messageId: number): Promise<void>
  setBadge(count: number): void
  onEvent(cb: (event: TalkEvent) => void): () => void
  avatarUrl(token: string, version: string): string
}
