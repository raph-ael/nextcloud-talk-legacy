// Asks the updater itself whether it would accept the installer as an update. The installed app
// compares the installer's signature with the publisher name in its app-update.yml (through
// PowerShell), and a name with non-ASCII letters has broken that comparison before. A build that
// fails here would install fine and then refuse every later update. Windows only; used by
// .github/workflows/desktop-windows.yml on a signed build:
//
//   node scripts/win-verify-update.cjs dist/win-unpacked/resources/app-update.yml dist/Wolkenfunk-Setup-X.Y.Z.exe
const fs = require('fs')
const path = require('path')
const yaml = require('js-yaml')
const { verifySignature } = require('electron-updater/out/windowsExecutableCodeSignatureVerifier')

const [ymlPath, installer] = process.argv.slice(2)
if (!ymlPath || !installer) {
  console.error('usage: win-verify-update.cjs app-update.yml installer.exe')
  process.exit(2)
}

const publisher = yaml.load(fs.readFileSync(ymlPath, 'utf8')).publisherName
const names = Array.isArray(publisher) ? publisher : publisher ? [publisher] : []
if (names.length === 0) {
  console.error('app-update.yml has no publisherName: the app would not check its updates')
  process.exit(1)
}

// The name itself stays out of the log.
const quiet = { info() {}, warn() {}, error() {}, debug() {} }
const file = path.resolve(installer)
const name = path.basename(installer)

async function main() {
  // The check has to be able to say no: with PowerShell out of order it lets everything pass.
  if ((await verifySignature(['Not The Publisher'], file, quiet)) == null) {
    throw new Error(`${name}: the updater's check accepts any name, so it proves nothing here`)
  }
  if ((await verifySignature(names, file, quiet)) != null) {
    throw new Error(`${name}: the updater would refuse this installer`)
  }
  console.log(`${name}: the updater accepts it as an update`)
}

main().catch((error) => {
  console.error(error.message)
  process.exit(1)
})
