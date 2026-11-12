// Screen sharing, main side (docs/calls.md „Bildschirmfreigabe“): what can be shared for the
// own picker (thumbnails from desktopCapturer), and which source the renderer's
// getDisplayMedia gets. On Linux/Wayland only the system's portal knows the screens, so the
// renderer skips its picker and the portal asks instead.
import { app, desktopCapturer, ipcMain, session, webContents, type BrowserWindow } from 'electron'
import type { ScreenSource } from '@shared/core'

/** A picked source is used by the next getDisplayMedia within this time, or not at all. */
const PICK_MS = 15_000

// Chromium captures Wayland screens over PipeWire (default on in current versions; set so it
// does not depend on that). Before `ready`, like every switch.
if (process.platform === 'linux') app.commandLine.appendSwitch('enable-features', 'WebRTCPipeWireCapturer')

/**
 * Linux in a Wayland session: Chromium captures through the portal (also under XWayland, it
 * goes by the session like WebRTC does), and only the portal knows the screens and windows.
 */
function waylandPortal(): boolean {
  return process.platform === 'linux' && (process.env.XDG_SESSION_TYPE ?? '').includes('wayland') && !!process.env.WAYLAND_DISPLAY
}

let names = new Map<string, string>()
let pick: { id: string | null; at: number } | null = null

/** IPC and the display-media handler; `window` is the Talk window (the only one allowed). */
export function installScreenShare(window: () => BrowserWindow | null): void {
  const own = (wc: Electron.WebContents | undefined) => !!wc && wc === window()?.webContents

  ipcMain.handle('talk:screenSources', async (e): Promise<ScreenSource[] | null> => {
    if (!own(e.sender) || waylandPortal()) return null
    const self = window()?.getMediaSourceId()
    const list = await desktopCapturer.getSources({
      types: ['screen', 'window'],
      thumbnailSize: { width: 320, height: 180 },
      fetchWindowIcons: true,
    })
    names = new Map(list.map((s) => [s.id, s.name]))
    return list
      .filter((s) => s.id !== self && s.name.trim() !== '')
      .map((s) => ({
        id: s.id,
        kind: s.id.startsWith('screen:') ? 'screen' : 'window',
        name: s.name,
        thumbnail: s.thumbnail.isEmpty() ? '' : `data:image/jpeg;base64,${s.thumbnail.toJPEG(75).toString('base64')}`,
        icon: s.appIcon && !s.appIcon.isEmpty() ? s.appIcon.toDataURL() : null,
      }))
  })

  ipcMain.handle('talk:chooseScreen', (e, sourceId: string | null) => {
    if (own(e.sender)) pick = { id: typeof sourceId === 'string' ? sourceId : null, at: Date.now() }
  })

  // Only our window, only right after a pick, only the picture (no system sound for now).
  session.defaultSession.setDisplayMediaRequestHandler((request, callback) => {
    const chosen = pick
    pick = null
    const from = request.frame ? webContents.fromFrame(request.frame) : undefined
    if (!chosen || Date.now() - chosen.at > PICK_MS || !own(from) || !request.videoRequested) {
      callback(null)
      return
    }
    if (chosen.id) {
      callback({ video: { id: chosen.id, name: names.get(chosen.id) ?? '' } })
      return
    }
    if (!waylandPortal()) {
      callback(null)
      return
    }
    // Wayland: this opens the portal; it returns what the user chose there (nothing: cancelled).
    desktopCapturer
      .getSources({ types: ['screen', 'window'], thumbnailSize: { width: 0, height: 0 } })
      .then((list) => callback(list[0] ? { video: list[0] } : null))
      .catch((err) => {
        console.warn('screen portal:', (err as Error).message)
        callback(null)
      })
  })
}
