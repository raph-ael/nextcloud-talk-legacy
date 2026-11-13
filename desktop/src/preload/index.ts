import { contextBridge, ipcRenderer, webUtils, type IpcRendererEvent } from 'electron'
import type { MediaCommand, TalkApi, TalkEvent } from '@shared/core'

const api: TalkApi = {
  account: () => ipcRenderer.invoke('talk:account'),
  login: (server) => ipcRenderer.invoke('talk:login', server),
  cancelLogin: () => ipcRenderer.invoke('talk:cancelLogin'),
  logout: () => ipcRenderer.invoke('talk:logout'),
  rooms: () => ipcRenderer.invoke('talk:rooms'),
  cachedChat: (token) => ipcRenderer.invoke('talk:cachedChat', token),
  openChat: (token) => ipcRenderer.invoke('talk:openChat', token),
  closeChat: (token) => ipcRenderer.invoke('talk:closeChat', token),
  loadOlder: (token, beforeId) => ipcRenderer.invoke('talk:loadOlder', token, beforeId),
  refreshRooms: () => ipcRenderer.invoke('talk:refreshRooms'),
  enqueueText: (token, text, replyTo) => ipcRenderer.invoke('talk:enqueueText', token, text, replyTo),
  enqueueFile: (token, file, caption, replyTo) => ipcRenderer.invoke('talk:enqueueFile', token, file, caption, replyTo),
  pending: (token) => ipcRenderer.invoke('talk:pending', token),
  retrySend: (referenceId) => ipcRenderer.invoke('talk:retrySend', referenceId),
  discard: (referenceId) => ipcRenderer.invoke('talk:discard', referenceId),
  networkChanged: () => ipcRenderer.send('talk:networkChanged'),
  stageData: (name, mime, data, width, height) => ipcRenderer.invoke('talk:stageData', name, mime, data, width, height),
  stagePath: (path, name, mime) => ipcRenderer.invoke('talk:stagePath', path, name, mime),
  unstage: (path) => ipcRenderer.invoke('talk:unstage', path),
  pathForFile: (file) => {
    try {
      return webUtils.getPathForFile(file)
    } catch {
      return ''
    }
  },
  voiceStart: (token, sampleRate) => ipcRenderer.invoke('talk:voiceStart', token, sampleRate),
  voicePush: (pcm) => ipcRenderer.invoke('talk:voicePush', pcm),
  voiceFinish: () => ipcRenderer.invoke('talk:voiceFinish'),
  voiceCancel: () => ipcRenderer.invoke('talk:voiceCancel'),
  voiceDraft: (token) => ipcRenderer.invoke('talk:voiceDraft', token),
  voiceDiscard: (token) => ipcRenderer.invoke('talk:voiceDiscard', token),
  voiceSend: (token, replyTo) => ipcRenderer.invoke('talk:voiceSend', token, replyTo),
  fetchFile: (path, size) => ipcRenderer.invoke('talk:fetchFile', path, size),
  audioInfo: (localPath) => ipcRenderer.invoke('talk:audioInfo', localPath),
  markRead: (token, messageId) => ipcRenderer.invoke('talk:markRead', token, messageId),
  setReaction: (token, messageId, emoji) => ipcRenderer.invoke('talk:setReaction', token, messageId, emoji),
  reactions: (token, messageId) => ipcRenderer.invoke('talk:reactions', token, messageId),
  canCreateGroups: () => ipcRenderer.invoke('talk:canCreateGroups'),
  searchUsers: (query, excludeToken) => ipcRenderer.invoke('talk:searchUsers', query, excludeToken),
  openDirect: (userId) => ipcRenderer.invoke('talk:openDirect', userId),
  createGroup: (name, userIds, photo) => ipcRenderer.invoke('talk:createGroup', name, userIds, photo),
  members: (token) => ipcRenderer.invoke('talk:members', token),
  addMembers: (token, userIds) => ipcRenderer.invoke('talk:addMembers', token, userIds),
  removeMember: (token, attendeeId) => ipcRenderer.invoke('talk:removeMember', token, attendeeId),
  setModerator: (token, attendeeId, moderator) => ipcRenderer.invoke('talk:setModerator', token, attendeeId, moderator),
  renameGroup: (token, name) => ipcRenderer.invoke('talk:renameGroup', token, name),
  setDescription: (token, description) => ipcRenderer.invoke('talk:setDescription', token, description),
  setGroupPhoto: (token, photo) => ipcRenderer.invoke('talk:setGroupPhoto', token, photo),
  setGroupLink: (token, on) => ipcRenderer.invoke('talk:setGroupLink', token, on),
  groupLink: (token) => ipcRenderer.invoke('talk:groupLink', token),
  setNotificationLevel: (token, level) => ipcRenderer.invoke('talk:setNotificationLevel', token, level),
  leaveNeedsModerator: (token) => ipcRenderer.invoke('talk:leaveNeedsModerator', token),
  leaveGroup: (token) => ipcRenderer.invoke('talk:leaveGroup', token),
  deleteGroup: (token) => ipcRenderer.invoke('talk:deleteGroup', token),
  setBadge: (count) => ipcRenderer.send('talk:setBadge', count),
  startCall: (token, silent) => ipcRenderer.invoke('talk:startCall', token, silent),
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
  recordingNotices: () => ipcRenderer.invoke('talk:recordingNotices'),
  shareRecording: (id) => ipcRenderer.invoke('talk:shareRecording', id),
  dismissRecording: (id) => ipcRenderer.invoke('talk:dismissRecording', id),
  userAvatarUrl: (userId) => `talk-avatar://user/${encodeURIComponent(userId)}`,
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
    const listener = (_e: IpcRendererEvent, event: TalkEvent) => cb(event)
    ipcRenderer.on('talk:event', listener)
    return () => ipcRenderer.removeListener('talk:event', listener)
  },
  avatarUrl: (token, version) => `talk-avatar://room/${encodeURIComponent(token)}?v=${encodeURIComponent(version)}`,
  previewUrl: (fileId, width, height) =>
    `talk-media://preview/${encodeURIComponent(fileId)}?w=${Math.round(width)}&h=${Math.round(height)}`,
  localFileUrl: (path) => `talk-media://local/file?p=${encodeURIComponent(path)}`,
}

contextBridge.exposeInMainWorld('talk', api)
