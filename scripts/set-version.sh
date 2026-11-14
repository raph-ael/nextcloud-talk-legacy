#!/usr/bin/env bash
# Sets one version for every app (docs/release.md):
#   scripts/set-version.sh 0.2.0 [development|stable]
#
#   desktop/package.json (+ lock)       "version"
#   android/app/build.gradle.kts        versionName, versionCode = X*10000 + Y*100 + Z
#   ios/project.yml                     MARKETING_VERSION, CURRENT_PROJECT_VERSION (same code)
#   …/org.wolkenfunk.Wolkenfunk.metainfo.xml   new <release> on top (AppStream, Flathub)
#
# It only edits files; committing and tagging is up to scripts/release.sh.
set -euo pipefail
cd "$(dirname "$0")/.."

VERSION="${1:?usage: scripts/set-version.sh X.Y.Z [development|stable]}"
TYPE="${2:-development}"
[[ "$VERSION" =~ ^([0-9]+)\.([0-9]+)\.([0-9]+)$ ]] || { echo "version must be X.Y.Z" >&2; exit 1; }
X=${BASH_REMATCH[1]} Y=${BASH_REMATCH[2]} Z=${BASH_REMATCH[3]}
(( Y < 100 && Z < 100 )) || { echo "minor and patch must be below 100" >&2; exit 1; }
CODE=$(( X * 10000 + Y * 100 + Z ))
[[ "$TYPE" == development || "$TYPE" == stable ]] || { echo "type is development or stable" >&2; exit 1; }

# Desktop
(cd desktop && npm version "$VERSION" --no-git-tag-version --allow-same-version >/dev/null)

# Android
sed -i -E "s/^(\s*versionCode = ).*/\1$CODE/; s/^(\s*versionName = ).*/\1\"$VERSION\"/" android/app/build.gradle.kts

# iOS
sed -i -E "s/^(\s*MARKETING_VERSION: ).*/\1\"$VERSION\"/; s/^(\s*CURRENT_PROJECT_VERSION: ).*/\1\"$CODE\"/" ios/project.yml

# AppStream: add a <release> unless this version is already listed.
META=desktop/packaging/linux/org.wolkenfunk.Wolkenfunk.metainfo.xml
if ! grep -q "<release version=\"$VERSION\"" "$META"; then
  python3 - "$META" "$VERSION" "$TYPE" "$(date -u +%F)" <<'EOF'
import sys
path, version, kind, date = sys.argv[1:]
text = open(path).read()
entry = (f'<releases>\n    <release version="{version}" date="{date}" type="{kind}">\n'
         f'      <description>\n        <p>Version {version}.</p>\n      </description>\n    </release>')
open(path, "w").write(text.replace("<releases>", entry, 1))
EOF
fi

echo "version $VERSION (code $CODE, $TYPE)"
grep -nE '^\s*(versionCode|versionName) = ' android/app/build.gradle.kts
grep -nE '^\s*(MARKETING_VERSION|CURRENT_PROJECT_VERSION):' ios/project.yml
grep -n '"version"' desktop/package.json
grep -n "<release version=\"$VERSION\"" "$META"
