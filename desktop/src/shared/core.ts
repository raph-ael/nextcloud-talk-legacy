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
  /** The user may react to messages (docs/chat.md „Reaktionen“). */
  canReact: boolean
  hasCall: boolean
  /** 0 default, 1 always, 2 mentions only, 3 never */
  notificationLevel: number
  avatarVersion: string
  /** A picture was set (it can be removed). docs/groups.md */
  hasCustomAvatar: boolean
  /** Owner or moderator: may edit the group and its members. */
  isModerator: boolean
  /** `false` when we are the last moderator: promote somebody first. */
  canLeave: boolean
  canDelete: boolean
  /** Group link on (room type 3). */
  isPublic: boolean
  /** Our categories it is in (docs/categories.md); the list uses `roomSections`. */
  tagIds: string[]
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

/** One person's reaction, for the "who reacted" list. */
export interface ReactionView {
  emoji: string
  actorType: string
  actorId: string
  name: string
  isOwn: boolean
  timestamp: number
}

/** A member of a group (docs/groups.md). */
export interface MemberView {
  /** For removeMember / setModerator. */
  attendeeId: number
  /** users, groups, guests, emails, federated_users … */
  actorType: string
  /** The user id for `users` (avatar, openDirect). */
  actorId: string
  name: string
  isSelf: boolean
  /** Owner or moderator. */
  isModerator: boolean
  /** The owner's rights cannot be changed and they cannot be removed. */
  isOwner: boolean
}

/** A user from the search ("Neue Nachricht", "Mitglieder hinzufügen"). */
export interface UserView {
  id: string
  name: string
  /** May be empty. */
  subline: string
}

/** One section of the chat list (docs/categories.md). */
export interface SectionView {
  /** For `toggleSection`; row keys are section id + token (a chat may be in several). */
  id: string
  kind: 'favorites' | 'custom' | 'other'
  /** Empty when it is the only section: then no header. */
  title: string
  collapsed: boolean
  /** Rows to show (collapsed: only the open chat, unread ones and ones with a call). */
  rooms: RoomView[]
  /** All chats in the section, hidden ones included. */
  total: number
}

/** A category on the page "Kategorien" (docs/categories.md). */
export interface CategoryView {
  id: string
  /** Only `custom` can be renamed, deleted and filled. */
  kind: 'favorites' | 'custom' | 'other'
  name: string
  roomCount: number
  collapsed: boolean
}

/** Opening or creating a conversation: its token, or a German error text. */
export interface OpenResult {
  token: string | null
  error: string | null
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
  /** A call was not answered here (also one that came in during another call). */
  | { type: 'callMissed'; token: string; roomName: string; callerName: string; isGroup: boolean }
  /** The whole list of recording notices (docs/calls.md, „Aufnahme“). */
  | { type: 'recordingsChanged'; items: RecordingNotice[] }
  /** Members or roles of a group changed: reload `members` if shown (docs/groups.md). */
  | { type: 'participantsChanged'; token: string }

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
  /** Their camera is on: show the video of `peerId` while frames arrive, else the avatar. */
  video: boolean
  /** Media engine peer carrying their audio and video. */
  peerId: string | null
}

/** Something to share (desktopCapturer source); the own Talk window is never one. */
export interface ScreenSource {
  id: string
  kind: 'screen' | 'window'
  name: string
  /** JPEG data URL, about 320×180. */
  thumbnail: string
  /** App icon of a window (PNG data URL), if known. */
  icon: string | null
}

/** A screen shared by another participant (docs/calls.md „Bildschirmfreigabe“). */
export interface CallScreen {
  /** Session of the participant sharing it. */
  sessionId: string
  name: string
  /** Receive-only peer carrying the screen: its video track is the screen. */
  peerId: string
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
  /** Our camera is on. */
  cameraOn: boolean
  /** We share our screen (`setScreenShare`). */
  screenSharing: boolean
  /** The conversation allows us to share our screen. */
  canShareScreen: boolean
  /** Screens shared by others, newest first (the first one takes the stage). */
  screens: CallScreen[]
  participants: CallParticipant[]
  endReason: EndReason | null
  /** Recording state, live from the server (also when somebody else started it). */
  recording: CallRecording
  /** We may start/stop a recording (moderator, recording server, in the call). */
  canRecord: boolean
  /** The server wants consent before we join; the core waits in `connecting`. */
  recordingConsentPending: boolean
}

export type CallRecording = 'off' | 'startingAudio' | 'startingVideo' | 'audio' | 'video' | 'failed'

export type RecordingNoticeKind =
  | 'recording'
  | 'transcript'
  | 'summary'
  | 'recordingFailed'
  | 'transcriptFailed'
  | 'summaryFailed'

/** A finished recording (or transcript, summary, failure) waiting for the user. */
export interface RecordingNotice {
  /** Pass to `shareRecording` / `dismissRecording`. */
  id: number
  token: string
  roomName: string
  kind: RecordingNoticeKind
  /** In the user's files; empty for an upload failure. */
  fileName: string
  /** Unix seconds. */
  timestamp: number
  /** „In Chat teilen“ is possible (otherwise only dismiss). */
  canShare: boolean
}

/** „Aufnahme verfügbar“ etc.: banner and system notification of a notice. */
export function recordingNoticeTitle(n: RecordingNotice): string {
  switch (n.kind) {
    case 'recording':
      return 'Aufnahme verfügbar'
    case 'transcript':
      return 'Transkript verfügbar'
    case 'summary':
      return 'Zusammenfassung verfügbar'
    case 'recordingFailed':
      return 'Aufnahme konnte nicht gespeichert werden'
    case 'transcriptFailed':
      return 'Transkript fehlgeschlagen'
    case 'summaryFailed':
      return 'Zusammenfassung fehlgeschlagen'
  }
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

export type PeerKind = 'publisher' | 'subscriber' | 'mesh' | 'screenPublisher' | 'screenSubscriber'
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
  | { cmd: 'setCameraEnabled'; enabled: boolean }
  | { cmd: 'startAudio'; callId: string }
  | { cmd: 'stopAudio'; callId: string }
  | { cmd: 'stopScreen' }

/** Renderer media engine → core. */
export type MediaReport =
  | { type: 'localDescription'; peerId: string; sdpType: SdpType; sdp: string }
  | { type: 'localCandidate'; peerId: string; candidate: IceCandidateInit }
  | { type: 'peerState'; peerId: string; state: PeerState }
  | { type: 'peerError'; peerId: string; message: string }
  | { type: 'audioLevel'; peerId: string; level: number }
  | { type: 'cameraState'; on: boolean }
  | { type: 'screenState'; on: boolean }

/** Events produced by the Electron main process itself. */
export type AppEvent =
  | { type: 'session'; state: 'loggedOut' }
  | { type: 'session'; state: 'ready'; account: AccountInfo }
  | { type: 'openRoom'; token: string }
  /** Keyboard shortcut or notification action of the incoming-call bar. */
  | { type: 'callAction'; action: 'accept' | 'decline' }
  /** The window entered or left fullscreen (our button, green button, F11, window menu). */
  | { type: 'windowFullscreen'; on: boolean }

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
  /**
   * Own reaction like Signal (one per person): `emoji` replaces it, the same emoji again
   * or `null` removes it. Shows at once via `messagesUpdated`. Resolves to a short German
   * error text to show, or `null` on success.
   */
  setReaction(token: string, messageId: number, emoji: string | null): Promise<string | null>
  /** Who reacted: own first, then newest first. */
  reactions(token: string, messageId: number): Promise<ReactionView[]>

  // --- categories (docs/categories.md); changes resolve to a German error text or null ---
  /** The chat list in sections; again on `roomsChanged`, a new search text or another open chat. */
  roomSections(query: string, activeToken: string | null): Promise<SectionView[]>
  /** Opens or closes a section (saved on the server; the list follows via `roomsChanged`). */
  toggleSection(id: string): Promise<string | null>
  /** The server keeps categories: offer "Kategorien verwalten". */
  categoriesSupported(): Promise<boolean>
  /** All categories in order, "Favoriten" and "Chats" included. */
  categories(): Promise<CategoryView[]>
  createCategory(name: string, tokens: string[]): Promise<{ id: string | null; error: string | null }>
  renameCategory(id: string, name: string): Promise<string | null>
  deleteCategory(id: string): Promise<string | null>
  /** -1 up, 1 down. */
  moveCategory(id: string, offset: number): Promise<string | null>
  /** Exactly these chats are in the category afterwards. */
  setCategoryRooms(id: string, tokens: string[]): Promise<string | null>

  // --- groups (docs/groups.md); changes resolve to a German error text or null ---
  /** Whether "Neue Gruppe" is offered. */
  canCreateGroups(): Promise<boolean>
  /** Users for "Neue Nachricht" (all listable ones for an empty query); with `excludeToken` without that group's members. */
  searchUsers(query: string, excludeToken: string | null): Promise<UserView[]>
  /** The one-to-one chat with a user (created when missing). */
  openDirect(userId: string): Promise<OpenResult>
  /** Creates a group; `photo`: any image bytes (the core crops it to a square). */
  createGroup(name: string, userIds: string[], photo: Uint8Array | null): Promise<OpenResult>
  /** Ourselves first, then moderators, then alphabetically. */
  members(token: string): Promise<MemberView[]>
  addMembers(token: string, userIds: string[]): Promise<string | null>
  removeMember(token: string, attendeeId: number): Promise<string | null>
  setModerator(token: string, attendeeId: number, moderator: boolean): Promise<string | null>
  renameGroup(token: string, name: string): Promise<string | null>
  setDescription(token: string, description: string): Promise<string | null>
  /** New picture (any image bytes), or null to remove it. */
  setGroupPhoto(token: string, photo: Uint8Array | null): Promise<string | null>
  setGroupLink(token: string, on: boolean): Promise<string | null>
  groupLink(token: string): Promise<string>
  /** 1 all messages, 2 mentions only, 3 off. */
  setNotificationLevel(token: string, level: number): Promise<string | null>
  /** Ask when "Gruppe verlassen" is clicked: true → "Neuen Moderator wählen" first. */
  leaveNeedsModerator(token: string): Promise<boolean>
  leaveGroup(token: string): Promise<string | null>
  deleteGroup(token: string): Promise<string | null>
  setBadge(count: number): void

  // --- calls ---
  /** Calls the conversation or joins its running call; `silent`: nobody is rung (tests). */
  startCall(token: string, silent: boolean): Promise<void>
  acceptCall(token: string): Promise<void>
  declineCall(token: string): Promise<void>
  hangUp(): Promise<void>
  setCallMuted(muted: boolean): Promise<void>
  /** Window fullscreen (call view); changes come back as `windowFullscreen` events. */
  setFullScreen(on: boolean): Promise<void>
  isFullScreen(): Promise<boolean>
  /** Camera on/off in the running call; false when the system refused the camera (macOS). */
  setCameraEnabled(on: boolean): Promise<boolean>
  /**
   * What can be shared, with thumbnails, for the own picker; `null`: the system shows its
   * own dialog instead (Linux/Wayland portal), `getDisplayMedia` right away; `false`: the
   * system does not allow screen recording (macOS), main already explained it.
   */
  screenSources(): Promise<ScreenSource[] | null | false>
  /** The source the next `getDisplayMedia` captures (`null`: the system dialog). */
  chooseScreen(sourceId: string | null): Promise<void>
  /** Screen sharing on/off in the running call: after `getDisplayMedia`, see `ScreenSource`. */
  setScreenShare(on: boolean): Promise<void>
  currentCall(): Promise<CallView | null>
  /** Only with `call.canRecord`; `video` records the Talk web view. */
  startRecording(video: boolean): Promise<void>
  stopRecording(): Promise<void>
  /** Answer to `call.recordingConsentPending`. */
  consentToRecording(): Promise<void>
  recordingNotices(): Promise<RecordingNotice[]>
  /** Posts the file into its conversation; the notice goes away. */
  shareRecording(id: number): Promise<void>
  /** Removes the notice (the file stays in the user's files). */
  dismissRecording(id: number): Promise<void>
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
