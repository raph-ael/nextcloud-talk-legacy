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

/** Longest voice message (the core stops accepting audio after it). */
export const VOICE_MAX_MS = 60 * 60 * 1000
/** Shorter recordings are discarded. */
export const VOICE_MIN_MS = 1000

/** A recorded voice message not sent yet (one per conversation, survives restarts). */
export interface VoiceDraft {
  id: string
  token: string
  /** MP3 file in the app's data folder. */
  path: string
  durationMs: number
  size: number
  /** 100 levels 0..255. */
  waveform: number[]
  createdAt: number
}

/** Returned by every pushed block of a recording. */
export interface RecordingStatus {
  durationMs: number
  /** Current level 0..1. */
  level: number
  /** One hour reached: the recording must be finished. */
  limitReached: boolean
}

export interface AudioInfo {
  durationMs: number
  waveform: number[]
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
  | { type: 'callChanged'; call: CallView | null }

// --- Calls (docs/calls.md, talk-core/src/call/mod.rs) ------------------------------

export type CallState = 'dialing' | 'ringing' | 'incoming' | 'connecting' | 'connected' | 'reconnecting' | 'ended'

export type EndReason =
  | { kind: 'hangup' }
  | { kind: 'remoteHangup' }
  | { kind: 'noAnswer' }
  | { kind: 'declined' }
  | { kind: 'answeredElsewhere' }
  | { kind: 'missed' }
  | { kind: 'failed'; code: string; message: string }

/** Another participant of the call (never ourselves). */
export interface CallParticipant {
  /** Signaling session id: stable key while they stay in the call. */
  sessionId: string
  /** Nextcloud user id; empty for guests. */
  userId: string
  name: string
  muted: boolean
  speaking: boolean
  /** Their audio reaches us. */
  connected: boolean
}

export interface CallView {
  /** Local id of this call (also the `callId` of `startAudio`). */
  id: string
  token: string
  roomName: string
  isGroup: boolean
  direction: 'outgoing' | 'incoming'
  state: CallState
  /** Who started the call (incoming); empty for outgoing. */
  callerName: string
  /** Unix ms when the call became `connected`. */
  startedAt: number | null
  muted: boolean
  participants: CallParticipant[]
  endReason: EndReason | null
}

export interface IceServerConfig {
  urls: string[]
  username?: string | null
  credential?: string | null
}

export interface IceCandidateInit {
  candidate: string
  sdpMid: string | null
  sdpMLineIndex: number
}

export type PeerKind = 'publisher' | 'subscriber' | 'mesh'
export type SdpType = 'offer' | 'answer'
export type PeerState = 'new' | 'connecting' | 'connected' | 'disconnected' | 'failed' | 'closed'

/** Core → renderer media engine, delivered in the order the core issued them. */
export type MediaCommand =
  | { cmd: 'createPeer'; peerId: string; kind: PeerKind; iceServers: IceServerConfig[] }
  | { cmd: 'createOffer'; peerId: string; iceRestart: boolean }
  | { cmd: 'createAnswer'; peerId: string }
  | { cmd: 'setRemoteDescription'; peerId: string; sdpType: SdpType; sdp: string }
  | { cmd: 'addRemoteCandidate'; peerId: string; candidate: IceCandidateInit }
  | { cmd: 'closePeer'; peerId: string }
  | { cmd: 'setMicrophoneEnabled'; enabled: boolean }
  | { cmd: 'startAudio'; callId: string }
  | { cmd: 'stopAudio'; callId: string }

/** Renderer media engine → core. */
export type MediaReport =
  | { type: 'localDescription'; peerId: string; sdpType: SdpType; sdp: string }
  | { type: 'localCandidate'; peerId: string; candidate: IceCandidateInit }
  | { type: 'peerState'; peerId: string; state: PeerState }
  | { type: 'peerError'; peerId: string; message: string }
  | { type: 'audioLevel'; peerId: string; level: number }

/** Events produced by the Electron main process itself. */
export type AppEvent =
  | { type: 'session'; state: 'loggedOut' }
  | { type: 'session'; state: 'ready'; account: AccountInfo }
  | { type: 'openRoom'; token: string }
  /** Keyboard shortcut or notification action of the incoming-call bar. */
  | { type: 'callAction'; action: 'accept' | 'decline' }

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
  /** Starts a voice recording for `token`; 16-bit mono PCM follows via `voicePush`. */
  voiceStart(token: string, sampleRate: number): Promise<void>
  /** One block of 16-bit little-endian PCM (written to disk as MP3 right away). */
  voicePush(pcm: Uint8Array): Promise<RecordingStatus>
  /** Ends the recording as draft (`null`: shorter than a second, discarded). */
  voiceFinish(): Promise<VoiceDraft | null>
  voiceCancel(): Promise<void>
  /** The unsent recording of a conversation (also one interrupted by a crash). */
  voiceDraft(token: string): Promise<VoiceDraft | null>
  voiceDiscard(token: string): Promise<void>
  /** Sends the draft through the outbox as a voice message. */
  voiceSend(token: string, replyTo: ReplyPreview | null): Promise<PendingMessage>
  /** Local copy of a shared file (downloaded once, kept for offline use). */
  fetchFile(path: string, size: number): Promise<string>
  /** Duration and waveform of a local audio file. */
  audioInfo(localPath: string): Promise<AudioInfo>
  markRead(token: string, messageId: number): Promise<void>
  setBadge(count: number): void

  // --- calls ---
  /** Calls the conversation or joins its running call; `silent`: nobody is rung (tests). */
  startCall(token: string, silent: boolean): Promise<void>
  acceptCall(token: string): Promise<void>
  declineCall(token: string): Promise<void>
  hangUp(): Promise<void>
  setCallMuted(muted: boolean): Promise<void>
  currentCall(): Promise<CallView | null>
  /** Avatar of a call participant by Nextcloud user id. */
  userAvatarUrl(userId: string): string
  /** Registers the media engine; commands arrive in order. Call once, then `mediaReady`. */
  onMediaCommand(cb: (cmd: MediaCommand) => void): () => void
  /** The engine is listening: main flushes queued commands (also after a reload). */
  mediaReady(): void
  mediaReport(report: MediaReport): void
  /** Attention for an incoming call (flash frame, silent notification); `null` stops it. */
  ringAttention(call: { roomName: string; callerName: string; isGroup: boolean } | null): void
  onEvent(cb: (event: TalkEvent) => void): () => void
  avatarUrl(token: string, version: string): string
  /** Server preview of a shared file, fitting into width×height (CSS px ×2 for HiDPI). */
  previewUrl(fileId: string, width: number, height: number): string
  /** A local file of the app (pending attachment, downloaded file, voice draft). */
  localFileUrl(path: string): string
}
