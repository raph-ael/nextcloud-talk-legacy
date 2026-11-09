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
  /** Replace the shown messages instead of merging (the cached copy was outdated). */
  reset: boolean
}

/** Outbox state of a message that is not delivered yet. */
export type PendingState = 'sending' | 'waiting' | 'failed'

/** Why a message finally failed (talk-core `SendError`). */
export type SendError = 'expired' | 'forbidden' | 'notFound' | 'tooLarge' | 'quotaExceeded' | 'fileMissing' | 'rejected'

export interface PendingMessage {
  message: MessageView
  state: PendingState
  /** Upload progress 0..1, negative when not uploading. */
  progress: number
  error: SendError | null
  /** Local copy of the attachment, shown until the server has it. */
  localPath: string | null
}

/** A file copied into the app's staging folder, ready for `enqueueFile`. */
export interface StagedFile {
  path: string
  name: string
  mime: string
  size: number
  width: number
  height: number
}

export type CoreEvent =
  | { type: 'roomsChanged'; rooms: RoomView[] }
  | { type: 'messagesAdded'; token: string; messages: MessageView[] }
  | { type: 'messagesUpdated'; token: string; messages: MessageView[] }
  | { type: 'readStatus'; token: string; lastCommonRead: number }
  | { type: 'connection'; online: boolean }
  | { type: 'authFailed' }
  | { type: 'outboxChanged'; token: string; items: PendingMessage[] }

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
  /** Newest cached messages, for the first frame of a chat (empty without cache). */
  cachedChat(token: string): Promise<ChatPage>
  openChat(token: string): Promise<ChatPage>
  closeChat(token: string): Promise<void>
  loadOlder(token: string, beforeId: number): Promise<ChatPage>
  refreshRooms(): Promise<void>
  /** Queues a text message in the persistent outbox (delivered in the background). */
  enqueueText(token: string, text: string, replyTo: ReplyPreview | null): Promise<PendingMessage>
  /** Queues a staged file; the outbox takes it over. */
  enqueueFile(token: string, file: StagedFile, caption: string, replyTo: ReplyPreview | null): Promise<PendingMessage>
  pending(token: string): Promise<PendingMessage[]>
  retrySend(referenceId: string): Promise<void>
  discard(referenceId: string): Promise<void>
  networkChanged(): void
  /** Writes bytes (a prepared image, pasted data) into the staging folder. */
  stageData(name: string, mime: string, data: Uint8Array, width: number, height: number): Promise<StagedFile>
  /** Copies a local file into the staging folder (the original stays untouched). */
  stagePath(path: string, name: string, mime: string): Promise<StagedFile>
  unstage(path: string): Promise<void>
  /** Local path of a dropped/pasted/picked file (empty for in-memory data). */
  pathForFile(file: File): string
  markRead(token: string, messageId: number): Promise<void>
  setBadge(count: number): void
  onEvent(cb: (event: TalkEvent) => void): () => void
  avatarUrl(token: string, version: string): string
  /** Server preview of a shared file, fitting into width×height (CSS px ×2 for HiDPI). */
  previewUrl(fileId: string, width: number, height: number): string
  /** A local file of the outbox (pending attachment). */
  localFileUrl(path: string): string
}
