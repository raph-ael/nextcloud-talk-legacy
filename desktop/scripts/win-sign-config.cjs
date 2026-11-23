// electron-builder config for a signed Windows build: the "build" field of package.json plus
// Azure Artifact Signing. Used by .github/workflows/desktop-windows.yml when signing is on:
//
//   npx electron-builder --win --publish never --config scripts/win-sign-config.cjs
//
// Account, profile and publisher come from the environment (CI secrets), never from the repo.
// The login itself reads AZURE_TENANT_ID, AZURE_CLIENT_ID and AZURE_CLIENT_SECRET.
const base = require('../package.json').build

function need(name) {
  const value = process.env[name]
  if (!value) throw new Error(`${name} is not set`)
  return value
}
for (const name of ['AZURE_TENANT_ID', 'AZURE_CLIENT_ID', 'AZURE_CLIENT_SECRET']) need(name)

module.exports = {
  ...base,
  win: {
    ...base.win,
    azureSignOptions: {
      // The name in the certificate (CN), exactly. It goes into the app, and the updater accepts
      // a later version only if its installer is signed with this name.
      publisherName: need('WIN_SIGN_PUBLISHER'),
      endpoint: need('WIN_SIGN_ENDPOINT'),
      codeSigningAccountName: need('WIN_SIGN_ACCOUNT'),
      certificateProfileName: need('WIN_SIGN_PROFILE'),
    },
  },
}
