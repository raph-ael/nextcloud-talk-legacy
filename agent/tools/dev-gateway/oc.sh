#!/bin/bash
# OpenClaw-CLI gegen das Test-Gateway (z. B. oc.sh channels status --probe, oc.sh config set …).
here="$(cd "$(dirname "$0")" && pwd)"
export OPENCLAW_HOME="$here/home"
exec "$here/node_modules/.bin/openclaw" "$@"
