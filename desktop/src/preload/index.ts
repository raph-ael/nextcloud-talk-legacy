import { contextBridge, ipcRenderer, type IpcRendererEvent } from 'electron'
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
  send: (token, text, referenceId, replyTo) => ipcRenderer.invoke('talk:send', token, text, referenceId, replyTo),
  markRead: (token, messageId) => ipcRenderer.invoke('talk:markRead', token, messageId),
  setBadge: (count) => ipcRenderer.send('talk:setBadge', count),
  onEvent: (cb) => {
    const listener = (_e: IpcRendererEvent, event: TalkEvent) => cb(event)
    ipcRenderer.on('talk:event', listener)
    return () => ipcRenderer.removeListener('talk:event', listener)
  },
  avatarUrl: (token, version) => `talk-avatar://room/${encodeURIComponent(token)}?v=${encodeURIComponent(version)}`,
}

contextBridge.exposeInMainWorld('talk', api)
