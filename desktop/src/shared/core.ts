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
  /** The person of a shared contact (a vCard file), never empty for one; empty for every other file. */
  contactName: string
}

/** A place shared into a chat (WGS 84, degrees); `name` may be empty. */
export interface GeoLocation {
  latitude: number
  longitude: number
  name: string
}

/** What a location is called where it has no name (docs/attachments.md). */
export function locationName(l: GeoLocation): string {
  return l.name.trim() || 'Standort'
}

/**
 * Short label for a file message ("📷 Foto", "📎 name.pdf", "👤 name" for a contact),
 * followed by its caption, or for a location ("📍 name"), whose `text` is its name again.
 */
export function withAttachment(
  text: string,
  a: FileAttachment | null | undefined,
  location?: GeoLocation | null,
): string {
  if (location) return `📍 ${locationName(location)}`
  if (!a) return text
  const label = a.contactName
    ? `👤 ${a.contactName}`
    : a.voiceMessage
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
  /** For a location; `text` is then its name. */
  location?: GeoLocation | null
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
  /** The user may write here, and so edit and delete own messages (docs/chat.md „Bearbeiten und Löschen“). */
  canChat: boolean
  /** The user may delete messages of others (moderator of a group). */
  canDeleteOthers: boolean
  hasCall: boolean
  /** Effective level: 1 always, 2 mentions only, 3 never (the core resolves the default). */
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
  location?: GeoLocation | null
}

/** A mention the user picked in the input field (docs/mentions.md). `label` has no „@“. */
export interface MentionPick {
  label: string
  /** What the server reads: `lena`, `group/garten`, `all` … */
  mentionId: string
}

export type MentionKind = 'user' | 'group' | 'team' | 'guest' | 'email' | 'federated' | 'all'

/** A mention in a message: `label` stands like this in `MessageView.text` („@Lena Brandt“). */
export interface MentionView {
  label: string
  kind: MentionKind
  /** The user id for `user`, else the mention id. */
  id: string
  /** The user's own name, or everybody. */
  isSelf: boolean
}

/** One row of the suggestions after an „@“. */
export interface MentionCandidate {
  mentionId: string
  /** The name without „@“. */
  label: string
  kind: MentionKind
  /** For `user`: whose picture the row shows. */
  userId: string | null
  /** Second line: „Gruppe“, „Team“, „Gast“, „Alle im Chat“, the id for a name that is there twice. */
  subline: string
}

/** A message's text for the input field, with its mentions. */
export interface EditDraft {
  text: string
  mentions: MentionPick[]
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
  /** For a location: drawn as a tile, `text` (its name) is not shown again. */
  location?: GeoLocation | null
  /** Can be deleted: by its author with `RoomView.canChat`, by others with `RoomView.canDeleteOthers`. */
  deletable: boolean
  /** Its author can edit the text (or caption) with `RoomView.canChat`, until `editUntil`. */
  editable: boolean
  /** Unix seconds; `null`: no limit. */
  editUntil: number | null
  /** The mentions in `text`, in its order (docs/mentions.md). */
  mentions: MentionView[]
  /** A foreign message that mentions the user (their name or everybody). */
  mentionsMe: boolean
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
  /** `archived` only in a search: archived chats whose name matches. No category behind it. */
  kind: 'favorites' | 'custom' | 'other' | 'groups' | 'direct' | 'archived'
  /** Empty when it is the only section: then no header. */
  title: string
  collapsed: boolean
  /** Rows to show: none when collapsed. */
  rooms: RoomView[]
  /** All chats in the section, hidden ones included. */
  total: number
  /** For the head while collapsed: unread messages of all its chats; muted ones do not count. */
  unread: number
  /** Show `unread` in grey like the number of a quiet chat: nothing in it counts for the badge. */
  unreadQuiet: boolean
  /** For the head while collapsed: an unread mention in one of its chats. */
  unreadMention: boolean
  /** For the head while collapsed: a running call in one of its chats. */
  hasCall: boolean
}

/** „Sortierung“ of the chats in every section (docs/categories.md). */
export type SortOrder = 'activity' | 'alphabetical'

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
  /** Newer messages follow this page: it is a window into the history (docs/search.md). */
  hasNewer: boolean
  /** For `loadNewer`; like `oldestId` it counts hidden messages too. */
  newestId: number | null
  /** Mentions of the user after the read marker that this page knows of (docs/mentions.md „Quellen“). */
  unreadMentions: number[]
}

/** A window into the history, or why there is none (German, for a toast). */
export type AroundResult = ChatPage | { error: string }

/** A piece of a search hit's excerpt; `hit` marks a search word. */
export interface SnippetPart {
  text: string
  hit: boolean
}

/** A message found by the search (docs/search.md). */
export interface SearchHit {
  token: string
  messageId: number
  timestamp: number
  roomName: string
  /** The sender, as in `MessageView` (`users`, `guests`, `bots` …). */
  actorType: string
  actorId: string
  actorName: string
  isOwn: boolean
  /** The sender goes before the excerpt („Du: “ or „{Name}: “). */
  showActor: boolean
  /** One line around the first match, ready to show. */
  snippet: SnippetPart[]
}

export interface SearchResults {
  /** Newest first. */
  hits: SearchHit[]
  /** In one chat: all matches, also beyond `hits`. In all chats: as many as `hits`. */
  total: number
  /** There are more matches than `hits`: ask again with a higher limit. */
  more: boolean
  /** Everything that can match is on the device: no need for `searchMessagesEverywhere`. */
  complete: boolean
  /** Chats whose history is loaded, and all of them. */
  chatsDone: number
  chatsTotal: number
}

/** Outbox state of a message that is not delivered yet. */
export type PendingState = 'sending' | 'waiting' | 'failed'
/** docs/connection.md */
export type ConnectionState = 'online' | 'connecting' | 'offline'

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
  /** docs/connection.md; a session starts as 'connecting'. */
  | { type: 'connection'; state: ConnectionState }
  | { type: 'authFailed' }
  | { type: 'outboxChanged'; token: string; items: PendingMessage[] }
  | { type: 'callChanged'; call: CallView | null }
  /** A call was not answered here (also one that came in during another call). */
  | { type: 'callMissed'; token: string; roomName: string; callerName: string; isGroup: boolean }
  /** The whole list of recording notices (docs/calls.md, „Aufnahme“). */
  | { type: 'recordingsChanged'; items: RecordingNotice[] }
  /** Members or roles of a group changed: reload `members` if shown (docs/groups.md). */
  | { type: 'participantsChanged'; token: string }
  /**
   * Chats with a new message that deserves a notification (docs/notifications.md), also the
   * one that is open. `sinceStart`: what came in while the app was closed.
   */
  | { type: 'messageNotices'; items: MessageNotice[]; sinceStart: boolean }
  /** The notifications of these chats are obsolete: read (here or elsewhere), archived, gone. */
  | { type: 'noticesCleared'; tokens: string[] }

/** One chat with something new, for a notification. */
export interface MessageNotice {
  token: string
  roomName: string
  isGroup: boolean
  mention: boolean
  unread: number
  /** The preview of the chat list; missing where the server shows none: „Neue Nachricht“. */
  message?: MessagePreview | null
}

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
  /** No account left: the login shows. */
  | { type: 'session'; state: 'loggedOut' }
  /**
   * The window shows `account` (docs/accounts.md): after the start, and on a switch, then
   * with that account's list right away (`rooms`), no loading state.
   */
  | { type: 'session'; state: 'ready'; account: AccountInfo; connection: ConnectionState; rooms?: RoomView[] }
  /** The list, the names or the numbers of the accounts changed. */
  | ({ type: 'accountsChanged' } & AccountsView)
  /** A short text for a toast (signed out, switching refused). */
  | { type: 'notice'; text: string }
  | { type: 'openRoom'; token: string }
  /** Keyboard shortcut or notification action of the incoming-call bar. */
  | { type: 'callAction'; action: 'accept' | 'decline' }
  /** The window entered or left fullscreen (our button, green button, F11, window menu). */
  | { type: 'windowFullscreen'; on: boolean }
  /** „Darstellung“ changed (also from another place, e.g. a second window in future). */
  | ({ type: 'appearanceChanged' } & Appearance)
  /** The tray check mark „Beim Schließen im Hintergrund weiterlaufen“ changed. */
  | { type: 'closeToTrayChanged'; on: boolean }
  /** „Einstellungen …“ in the macOS app menu. */
  | { type: 'openSettings' }

/** „Design“ in „Darstellung“ (docs/settings.md). */
export type Theme = 'system' | 'light' | 'dark'

export interface Appearance {
  theme: Theme
  /** Accent id (desktop/src/renderer/src/accents.ts). */
  accent: string
  /** „Ansicht“ (docs/compact-view.md). */
  layout: Layout
  /** „Gruppen und Direktchats trennen“ (docs/categories.md). */
  splitChats: boolean
}

/** „Sprechblasen“ or „Kompakt“ (docs/compact-view.md). */
export type Layout = 'bubbles' | 'compact'

/** „Über Wolkenfunk“. */
export interface AppInfo {
  version: string
  core: string
  /** The self-updater runs here (AppImage, Windows): offer „Nach Updates suchen“. */
  canUpdate: boolean
}

/** Result of „Nach Updates suchen“. */
export type UpdateCheck = { state: 'current' } | { state: 'ready'; version: string }

/**
 * `accountId`: the account the event belongs to. Only the shown account's events reach the
 * window; `callChanged` comes from whichever account has the call.
 */
export type TalkEvent = (CoreEvent | AppEvent) & { accountId?: string }

export interface AccountInfo {
  /** Local account id (docs/accounts.md). */
  id: string
  server: string
  /** Login name. */
  user: string
  userId: string
  /** Display name from the server, else the user id. */
  name: string
}

/** One row of the account menu. */
export interface AccountRow {
  id: string
  name: string
  /** Without `https://`. */
  server: string
  userId: string
  /** `badge().chats`: chats with something new that counts. */
  chats: number
}

export interface AccountsView {
  /** In the order of signing in. */
  accounts: AccountRow[]
  active: string | null
}

/** API exposed by the preload script as `window.talk`. */
export interface TalkApi {
  /**
   * Every call below that concerns chats runs in the account the window shows (the preload
   * passes its id); answers that arrive after a switch are dropped and never settle.
   */
  /** The shown account and its cached list. */
  account(): Promise<{ account: AccountInfo; connection: ConnectionState; rooms: RoomView[] } | null>
  accounts(): Promise<AccountsView>
  /** A German text when it is refused (during a call). */
  switchAccount(id: string): Promise<string | null>
  /**
   * First sign-in and „Konto hinzufügen“: opens the browser, resolves once the account is
   * signed in and shown; `duplicate`: it was signed in already and is shown now.
   */
  addAccount(server: string): Promise<'added' | 'duplicate'>
  cancelLogin(): Promise<void>
  /** Signs the shown account out on this device. */
  logout(): Promise<void>
  rooms(): Promise<RoomView[]>
  /** Newest cached messages, for the first frame of a chat (empty without cache). */
  cachedChat(token: string): Promise<ChatPage>
  openChat(token: string): Promise<ChatPage>
  closeChat(token: string): Promise<void>
  loadOlder(token: string, beforeId: number): Promise<ChatPage>
  /** The window around a message (docs/search.md „Chat an einer Nachricht öffnen“). */
  messagesAround(token: string, messageId: number): Promise<AroundResult>
  /** The next messages of such a window, after `ChatPage.newestId`. */
  loadNewer(token: string, afterId: number): Promise<ChatPage>
  /** Searches on the device, in one chat or (`token` null) in all; answers within milliseconds. */
  searchMessages(query: string, token: string | null, limit: number): Promise<SearchResults>
  /** The same with the server's hits added; only while `complete` is false. */
  searchMessagesEverywhere(query: string, token: string | null, limit: number): Promise<SearchResults>
  refreshRooms(): Promise<void>
  /** Queues a text message in the persistent outbox (delivered in the background). */
  enqueueText(token: string, text: string, mentions: MentionPick[], replyTo: ReplyPreview | null): Promise<PendingMessage>
  /** Queues a staged file; the outbox takes it over. `mentions` belong to the caption. */
  enqueueFile(
    token: string,
    file: StagedFile,
    caption: string,
    mentions: MentionPick[],
    replyTo: ReplyPreview | null,
  ): Promise<PendingMessage>
  pending(token: string): Promise<PendingMessage[]>
  retrySend(referenceId: string): Promise<void>
  discard(referenceId: string): Promise<void>
  networkChanged(): void
  /** The system has no network at all (docs/connection.md). */
  networkLost(): void
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
  /**
   * Text and mentions of a message for the input field when editing it; `null`: start from
   * `MessageView.text` and `MessageView.mentions`.
   */
  editSource(token: string, messageId: number): Promise<EditDraft | null>
  /** Edits an own message. Resolves to a German error text to show, or `null` (docs/chat.md). */
  editMessage(token: string, messageId: number, text: string, mentions: MentionPick[]): Promise<string | null>
  /**
   * Who can be mentioned in a chat, for what was typed after the „@“ (docs/mentions.md):
   * answers from memory, so it is asked on every keystroke. Empty in a one-to-one chat.
   */
  mentionCandidates(token: string, query: string, limit: number): Promise<MentionCandidate[]>
  /** The one-to-one chat with a person when there is one already; creates nothing. */
  existingDirect(userId: string): Promise<string | null>
  /** Deletes a message for everybody. Resolves to a German error text to show, or `null`. */
  deleteMessage(token: string, messageId: number): Promise<string | null>
  /** Who reacted: own first, then newest first. */
  reactions(token: string, messageId: number): Promise<ReactionView[]>

  // --- categories (docs/categories.md); changes resolve to a German error text or null ---
  /** The chat list in sections; again on `roomsChanged`, a new search text or another open chat. */
  /** `split`: „Gruppen“ and „Direktchats“ instead of „Chats“ (the device setting). */
  roomSections(query: string, activeToken: string | null, split: boolean): Promise<SectionView[]>
  /** Opens or closes a section (saved on the server; the list follows via `roomsChanged`). */
  toggleSection(id: string): Promise<string | null>
  /** „Sortierung“ of the account shown; read again on `roomsChanged`. */
  sortOrder(): Promise<SortOrder>
  /** Saved on the server where it knows the setting; resolves to a German error or null. */
  setSortOrder(order: SortOrder): Promise<string | null>
  /** The server keeps categories: offer „Kategorien“ in the settings. */
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

  // --- chat menu (docs/settings.md); a German error text or null --------------------
  /** Shows at once (`roomsChanged`), taken back on failure. */
  setFavorite(token: string, on: boolean): Promise<string | null>
  /** The chat is in exactly these own categories afterwards. */
  setRoomCategories(token: string, ids: string[]): Promise<string | null>
  /** „Als gelesen markieren“. */
  markRoomRead(token: string): Promise<string | null>

  // --- settings (docs/settings.md): this device only --------------------------------
  appearance(): Promise<Appearance>
  /** Applies at once everywhere; the main process answers with `appearanceChanged`. */
  setAppearance(theme: Theme, accent: string): Promise<void>
  /** Both answer with `appearanceChanged` too. */
  setLayout(layout: Layout): Promise<void>
  setSplitChats(on: boolean): Promise<void>
  closeToTray(): Promise<boolean>
  setCloseToTray(on: boolean): Promise<void>
  /** „Beim Anmelden starten“ (docs/notifications.md); null where there is no such switch (Flatpak). */
  openAtLogin(): Promise<boolean | null>
  setOpenAtLogin(on: boolean): Promise<void>
  appInfo(): Promise<AppInfo>
  /** Rejects when the check fails. */
  checkForUpdates(): Promise<UpdateCheck>
  /** Ask when "Gruppe verlassen" is clicked: true → "Neuen Moderator wählen" first. */
  leaveNeedsModerator(token: string): Promise<boolean>
  leaveGroup(token: string): Promise<string | null>
  deleteGroup(token: string): Promise<string | null>

  // --- calls: after the start everything goes to the account of the call ---
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
  currentCall(): Promise<{ call: CallView; account: string } | null>
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
  /** Avatar by Nextcloud user id; `account`: another than the shown one (call, account menu). */
  userAvatarUrl(userId: string, account?: string): string
  /** Registers the media engine; commands arrive in order. Call once, then `mediaReady`. */
  onMediaCommand(cb: (cmd: MediaCommand) => void): () => void
  /** The engine is listening: main flushes queued commands (also after a reload). */
  mediaReady(): void
  mediaReport(report: MediaReport): void
  /** Attention for an incoming call (flash frame, silent notification); `null` stops it. */
  ringAttention(call: { roomName: string; callerName: string; isGroup: boolean } | null): void
  onEvent(cb: (event: TalkEvent) => void): () => void
  avatarUrl(token: string, version: string, account?: string): string
  /** Server preview of a shared file, fitting into width×height (CSS px ×2 for HiDPI). */
  previewUrl(fileId: string, width: number, height: number): string
  /** A local file of the app (pending attachment, downloaded file, voice draft). */
  localFileUrl(path: string): string
}
