#!/bin/sh
# Start script inside the Flatpak (/app/bin/wolkenfunk). Zypak replaces Chromium's SUID/namespace
# sandbox, which cannot work inside the Flatpak sandbox, with Flatpak's own sub-sandboxes.
# Electron picks Wayland or X11 by itself (ozone auto), nothing to pass for that.
export TMPDIR="$XDG_RUNTIME_DIR/app/$FLATPAK_ID"
exec zypak-wrapper /app/main/wolkenfunk "$@"
