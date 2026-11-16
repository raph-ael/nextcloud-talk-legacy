import { contextBridge, ipcRenderer, webUtils, type IpcRendererEvent } from 'electron'
import type { MediaCommand, TalkApi, TalkEvent } from '@shared/core'

/**
 * The account the window shows (docs/accounts.md): it goes with every call for chats, and
 * main runs the call in exactly that account. It changes with the `session` event, before
 * the window hears of it.
 */
let shown: string | null = null

/** Answers that arrive after a switch belong to the account before: dropped, never settle. */
function call<T>(channel: string, ...args: unknown[]): Promise<T> {
  const id = shown
  const never = new Promise<never>(() => {})
  return ipcRenderer.invoke(channel, id, ...args).then(
    (v: T) => (id === shown ? v : never),
    (err: unknown) => (id === shown ? Promise.reject(err) : never),
  )
}

const enc = encodeURIComponent

const api: TalkApi = {
  account: () =>
    ipcRenderer.invoke('talk:account').then((a: Awaited<ReturnType<TalkApi['account']>>) => {
      shown = a?.account.id ?? null
      return a
    }),
  accounts: () => ipcRenderer.invoke('talk:accounts'),
  switchAccount: (id) => ipcRenderer.invoke('talk:switchAccount', id),
  addAccount: (server) => ipcRenderer.invoke('talk:addAccount', server),
  cancelLogin: () => ipcRenderer.invoke('talk:cancelLogin'),
  // Carries the account, but its answer comes after the switch it causes: not dropped.
  logout: () => ipcRenderer.invoke('talk:logout', shown),
  rooms: () => call('talk:rooms'),
  cachedChat: (token) => call('talk:cachedChat', token),
  openChat: (token) => call('talk:openChat', token),
  closeChat: (token) => call('talk:closeChat', token),
  loadOlder: (token, beforeId) => call('talk:loadOlder', token, beforeId),
  refreshRooms: () => call('talk:refreshRooms'),
  enqueueText: (token, text, replyTo) => call('talk:enqueueText', token, text, replyTo),
  enqueueFile: (token, file, caption, replyTo) => call('talk:enqueueFile', token, file, caption, replyTo),
  pending: (token) => call('talk:pending', token),
  retrySend: (referenceId) => call('talk:retrySend', referenceId),
  discard: (referenceId) => call('talk:discard', referenceId),
  networkChanged: () => ipcRenderer.send('talk:networkChanged'),
  networkLost: () => ipcRenderer.send('talk:networkLost'),
  stageData: (name, mime, data, width, height) => call('talk:stageData', name, mime, data, width, height),
  stagePath: (path, name, mime) => call('talk:stagePath', path, name, mime),
  unstage: (path) => call('talk:unstage', path),
  pathForFile: (file) => {
    try {
      return webUtils.getPathForFile(file)
    } catch {
      return ''
    }
  },
  voiceStart: (token, sampleRate) => call('talk:voiceStart', token, sampleRate),
  voicePush: (pcm) => ipcRenderer.invoke('talk:voicePush', pcm),
  voiceFinish: () => ipcRenderer.invoke('talk:voiceFinish'),
  voiceCancel: () => ipcRenderer.invoke('talk:voiceCancel'),
  voiceDraft: (token) => call('talk:voiceDraft', token),
  voiceDiscard: (token) => call('talk:voiceDiscard', token),
  voiceSend: (token, replyTo) => call('talk:voiceSend', token, replyTo),
  fetchFile: (path, size) => call('talk:fetchFile', path, size),
  audioInfo: (localPath) => call('talk:audioInfo', localPath),
  markRead: (token, messageId) => call('talk:markRead', token, messageId),
  setReaction: (token, messageId, emoji) => call('talk:setReaction', token, messageId, emoji),
  reactions: (token, messageId) => call('talk:reactions', token, messageId),
  roomSections: (query, activeToken) => call('talk:roomSections', query, activeToken),
  toggleSection: (id) => call('talk:toggleSection', id),
  categoriesSupported: () => call('talk:categoriesSupported'),
  categories: () => call('talk:categories'),
  createCategory: (name, tokens) => call('talk:createCategory', name, tokens),
  renameCategory: (id, name) => call('talk:renameCategory', id, name),
  deleteCategory: (id) => call('talk:deleteCategory', id),
  moveCategory: (id, offset) => call('talk:moveCategory', id, offset),
  setCategoryRooms: (id, tokens) => call('talk:setCategoryRooms', id, tokens),
  canCreateGroups: () => call('talk:canCreateGroups'),
  searchUsers: (query, excludeToken) => call('talk:searchUsers', query, excludeToken),
  openDirect: (userId) => call('talk:openDirect', userId),
  createGroup: (name, userIds, photo) => call('talk:createGroup', name, userIds, photo),
  members: (token) => call('talk:members', token),
  addMembers: (token, userIds) => call('talk:addMembers', token, userIds),
  removeMember: (token, attendeeId) => call('talk:removeMember', token, attendeeId),
  setModerator: (token, attendeeId, moderator) => call('talk:setModerator', token, attendeeId, moderator),
  renameGroup: (token, name) => call('talk:renameGroup', token, name),
  setDescription: (token, description) => call('talk:setDescription', token, description),
  setGroupPhoto: (token, photo) => call('talk:setGroupPhoto', token, photo),
  setGroupLink: (token, on) => call('talk:setGroupLink', token, on),
  groupLink: (token) => call('talk:groupLink', token),
  setNotificationLevel: (token, level) => call('talk:setNotificationLevel', token, level),
  setFavorite: (token, on) => call('talk:setFavorite', token, on),
  setRoomCategories: (token, ids) => call('talk:setRoomCategories', token, ids),
  markRoomRead: (token) => call('talk:markRoomRead', token),
  appearance: () => ipcRenderer.invoke('talk:appearance'),
  setAppearance: (theme, accent) => ipcRenderer.invoke('talk:setAppearance', theme, accent),
  closeToTray: () => ipcRenderer.invoke('talk:closeToTray'),
  setCloseToTray: (on) => ipcRenderer.invoke('talk:setCloseToTray', on),
  openAtLogin: () => ipcRenderer.invoke('talk:openAtLogin'),
  setOpenAtLogin: (on) => ipcRenderer.invoke('talk:setOpenAtLogin', on),
  appInfo: () => ipcRenderer.invoke('talk:appInfo'),
  checkForUpdates: () => ipcRenderer.invoke('talk:checkForUpdates'),
  leaveNeedsModerator: (token) => call('talk:leaveNeedsModerator', token),
  leaveGroup: (token) => call('talk:leaveGroup', token),
  deleteGroup: (token) => call('talk:deleteGroup', token),
  startCall: (token, silent) => call('talk:startCall', token, silent),
  acceptCall: (token) => ipcRenderer.invoke('talk:acceptCall', token),
  declineCall: (token) => ipcRenderer.invoke('talk:declineCall', token),
  hangUp: () => ipcRenderer.invoke('talk:hangUp'),
  setCallMuted: (muted) => ipcRenderer.invoke('talk:setCallMuted', muted),
  setCameraEnabled: (on) => ipcRenderer.invoke('talk:setCameraEnabled', on),
  screenSources: () => ipcRenderer.invoke('talk:screenSources'),
  chooseScreen: (sourceId) => ipcRenderer.invoke('talk:chooseScreen', sourceId),
  setScreenShare: (on) => ipcRenderer.invoke('talk:setScreenShare', on),
  setFullScreen: (on) => ipcRenderer.invoke('talk:setFullScreen', on),
  isFullScreen: () => ipcRenderer.invoke('talk:isFullScreen'),
  currentCall: () => ipcRenderer.invoke('talk:currentCall'),
  startRecording: (video) => ipcRenderer.invoke('talk:startRecording', video),
  stopRecording: () => ipcRenderer.invoke('talk:stopRecording'),
  consentToRecording: () => ipcRenderer.invoke('talk:consentToRecording'),
  recordingNotices: () => call('talk:recordingNotices'),
  shareRecording: (id) => call('talk:shareRecording', id),
  dismissRecording: (id) => call('talk:dismissRecording', id),
  userAvatarUrl: (userId, account) => `talk-avatar://${account ?? shown}/user/${enc(userId)}`,
  onMediaCommand: (cb) => {
    const listener = (_e: IpcRendererEvent, cmd: MediaCommand) => cb(cmd)
    ipcRenderer.on('talk:media', listener)
    return () => ipcRenderer.removeListener('talk:media', listener)
  },
  mediaReady: () => ipcRenderer.send('talk:mediaReady'),
  // One channel, `send` (not invoke): reports reach the core in the order they were made.
  mediaReport: (report) => ipcRenderer.send('talk:mediaReport', report),
  ringAttention: (call) => ipcRenderer.send('talk:ringAttention', call),
  onEvent: (cb) => {
    const listener = (_e: IpcRendererEvent, event: TalkEvent) => {
      // Late events of the account before a switch (main sends only the shown one's).
      if (event.accountId && event.accountId !== shown && event.type !== 'callChanged') return
      cb(event)
    }
    ipcRenderer.on('talk:event', listener)
    return () => ipcRenderer.removeListener('talk:event', listener)
  },
  avatarUrl: (token, version, account) => `talk-avatar://${account ?? shown}/room/${enc(token)}?v=${enc(version)}`,
  previewUrl: (fileId, width, height) =>
    `talk-media://${shown}/preview/${enc(fileId)}?w=${Math.round(width)}&h=${Math.round(height)}`,
  localFileUrl: (path) => `talk-media://${shown}/local/file?p=${enc(path)}`,
}

// Before any listener of the window: from here on calls carry the new account.
ipcRenderer.on('talk:event', (_e, event: TalkEvent) => {
  if (event.type === 'session') shown = event.state === 'ready' ? event.account.id : null
})

contextBridge.exposeInMainWorld('talk', api)
