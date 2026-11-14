// Self-update for the AppImage and the Windows installer (NSIS): neither has a package manager
// behind it. The .deb gets its updates from the APT repository, the Flatpak from Flathub, and
// macOS waits for a Developer ID signature; those must never update themselves.
// Feeds (publish config in package.json): https://pkg.wolkenfunk.org/appimage/latest-linux.yml
// and https://pkg.wolkenfunk.org/windows/latest.yml.

import { app } from 'electron'
import { autoUpdater } from 'electron-updater'
import type { UpdateCheck } from '@shared/core'

const CHECK_EVERY_MS = 6 * 60 * 60 * 1000
const NOTIFICATION = {
  title: 'Wolkenfunk {version} ist bereit',
  body: 'Das Update wird beim nächsten Beenden installiert.',
}

let active = false
/** Version downloaded and waiting for the next quit. */
let readyVersion: string | null = null

/** „Nach Updates suchen“ in „Über Wolkenfunk“ only where this updater runs. */
export function updatesActive(): boolean {
  return active
}

export function installAutoUpdate(): void {
  if (!app.isPackaged) return
  const appImage = process.platform === 'linux' && !!process.env.APPIMAGE
  if (!appImage && process.platform !== 'win32') return
  active = true
  // Downloads in the background, installs when the app quits (the tray keeps it running, so
  // the notification says when).
  autoUpdater.autoDownload = true
  autoUpdater.autoInstallOnAppQuit = true
  autoUpdater.logger = null
  autoUpdater.on('error', (err) => console.warn('update check failed:', err.message))
  autoUpdater.on('update-downloaded', (info) => {
    readyVersion = info.version
  })
  const check = (): void => {
    autoUpdater
      .checkForUpdatesAndNotify(NOTIFICATION)
      .catch((err: Error) => console.warn('update check failed:', err.message))
  }
  check()
  setInterval(check, CHECK_EVERY_MS).unref()
}

/**
 * „Nach Updates suchen“: resolves once it is clear – no newer version, or the newer one is
 * downloaded (then the usual notification shows as well). Rejects when the check fails.
 */
export async function checkForUpdatesNow(): Promise<UpdateCheck> {
  if (!active) return { state: 'current' }
  if (readyVersion) return { state: 'ready', version: readyVersion }
  const result = await autoUpdater.checkForUpdatesAndNotify(NOTIFICATION)
  if (!result?.isUpdateAvailable) return { state: 'current' }
  await result.downloadPromise
  return { state: 'ready', version: result.updateInfo.version }
}
