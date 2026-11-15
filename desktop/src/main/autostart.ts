// „Beim Anmelden starten“ (docs/notifications.md „Desktop“): Windows and macOS keep a login
// item, Linux gets an entry in ~/.config/autostart. A run started that way shows no window.

import { app } from 'electron'
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { homedir } from 'node:os'
import { dirname, join } from 'node:path'

/** Marks a run that the system started at login (Linux, Windows). */
const HIDDEN = '--hidden'

/** A Flatpak cannot write into ~/.config/autostart: no switch there. */
export function autostartAvailable(): boolean {
  return !(process.platform === 'linux' && (!!process.env.FLATPAK_ID || existsSync('/.flatpak-info')))
}

/** This run was started at login, not by the user. */
export function startedAtLogin(): boolean {
  if (process.platform === 'darwin') return app.getLoginItemSettings().wasOpenedAtLogin
  return process.argv.includes(HIDDEN)
}

/** Program and arguments that start this app again; a dev run needs its folder. */
function command(): string[] {
  const program = process.env.APPIMAGE || process.execPath
  return [program, ...(app.isPackaged ? [] : [app.getAppPath()]), HIDDEN]
}

export function setAutostart(on: boolean): void {
  if (process.platform === 'linux') {
    setLinuxEntry(on)
  } else if (process.platform === 'win32') {
    const [path, ...args] = command()
    app.setLoginItemSettings({ openAtLogin: on, path, args })
  } else {
    // macOS knows no arguments; `wasOpenedAtLogin` tells such a run apart.
    app.setLoginItemSettings({ openAtLogin: on })
  }
}

// --- Linux: XDG autostart -----------------------------------------------------------------

function linuxEntry(): string {
  const config = process.env.XDG_CONFIG_HOME || join(homedir(), '.config')
  return join(config, 'autostart', 'org.wolkenfunk.Wolkenfunk.desktop')
}

/**
 * One argument of an `Exec` line (Desktop Entry Specification): quoted, the reserved
 * characters escaped, then every backslash doubled as in any string value.
 */
export function execArg(arg: string): string {
  const quoted = `"${arg.replace(/(["`$\\])/g, '\\$1')}"`
  return quoted.replace(/\\/g, '\\\\').replace(/%/g, '%%')
}

function setLinuxEntry(on: boolean): void {
  const file = linuxEntry()
  if (!on) {
    rmSync(file, { force: true })
    return
  }
  const entry = [
    '[Desktop Entry]',
    'Type=Application',
    'Name=Wolkenfunk',
    `Exec=${command().map(execArg).join(' ')}`,
    'Icon=wolkenfunk',
    'Terminal=false',
    'X-GNOME-Autostart-enabled=true',
    '',
  ].join('\n')
  try {
    if (readFileSync(file, 'utf8') === entry) return
  } catch {
    // Not there yet.
  }
  mkdirSync(dirname(file), { recursive: true })
  writeFileSync(file, entry)
}
