import { contextBridge, ipcRenderer, webUtils, type IpcRendererEvent } from 'electron'
import type { TalkApi, TalkEvent } from '@shared/core'

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
  markRead: (token, messageId) => ipcRenderer.invoke('talk:markRead', token, messageId),
  setBadge: (count) => ipcRenderer.send('talk:setBadge', count),
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
