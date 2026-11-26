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
verifySignature(names, path.resolve(installer), quiet).then(
  (failure) => {
    if (failure != null) {
      console.error(`${path.basename(installer)}: the updater would refuse this installer`)
      process.exit(1)
    }
    console.log(`${path.basename(installer)}: the updater accepts it as an update`)
  },
  (error) => {
    console.error(`the updater's check could not run: ${error.message}`)
    process.exit(1)
  },
)
