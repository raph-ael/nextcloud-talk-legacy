// Self-update, only for the AppImage: it has no package manager behind it. The .deb gets its
// updates from the APT repository, the Flatpak from Flathub; both must never update themselves.
// Feed: https://pkg.wolkenfunk.org/appimage/latest-linux.yml (publish config in package.json).

import { app } from 'electron'
import { autoUpdater } from 'electron-updater'

const CHECK_EVERY_MS = 6 * 60 * 60 * 1000

export function installAutoUpdate(): void {
  if (!app.isPackaged || process.platform !== 'linux' || !process.env.APPIMAGE) return
  // Downloads in the background, installs when the app quits (the tray keeps it running, so
  // the notification says when).
  autoUpdater.autoDownload = true
  autoUpdater.autoInstallOnAppQuit = true
  autoUpdater.logger = null
  autoUpdater.on('error', (err) => console.warn('update check failed:', err.message))
  const check = (): void => {
    autoUpdater
      .checkForUpdatesAndNotify({
        title: 'Wolkenfunk {version} ist bereit',
        body: 'Das Update wird beim nächsten Beenden installiert.',
      })
      .catch((err: Error) => console.warn('update check failed:', err.message))
  }
  check()
  setInterval(check, CHECK_EVERY_MS).unref()
}
