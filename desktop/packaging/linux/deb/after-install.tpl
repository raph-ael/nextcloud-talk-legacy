#!/bin/bash

if type update-alternatives >/dev/null 2>&1; then
    # Remove previous link if it doesn't use update-alternatives
    if [ -L '/usr/bin/${executable}' -a -e '/usr/bin/${executable}' -a "`readlink '/usr/bin/${executable}'`" != '/etc/alternatives/${executable}' ]; then
        rm -f '/usr/bin/${executable}'
    fi
    update-alternatives --install '/usr/bin/${executable}' '${executable}' '/opt/${sanitizedProductName}/${executable}' 100 || ln -sf '/opt/${sanitizedProductName}/${executable}' '/usr/bin/${executable}'
else
    ln -sf '/opt/${sanitizedProductName}/${executable}' '/usr/bin/${executable}'
fi

# Check if user namespaces are supported by the kernel and working with a quick test:
if ! { [[ -L /proc/self/ns/user ]] && unshare --user true; }; then
    # Use SUID chrome-sandbox only on systems without user namespaces:
    chmod 4755 '/opt/${sanitizedProductName}/chrome-sandbox' || true
else
    chmod 0755 '/opt/${sanitizedProductName}/chrome-sandbox' || true
fi

if hash update-mime-database 2>/dev/null; then
    update-mime-database /usr/share/mime || true
fi

if hash update-desktop-database 2>/dev/null; then
    update-desktop-database /usr/share/applications || true
fi

# Install apparmor profile. (Ubuntu 24+)
# First check if the version of AppArmor running on the device supports our profile.
# This is in order to keep backwards compatibility with Ubuntu 22.04 which does not support abi/4.0.
# In that case, we just skip installing the profile since the app runs fine without it on 22.04.
#
# Those apparmor_parser flags are akin to performing a dry run of loading a profile.
# https://wiki.debian.org/AppArmor/HowToUse#Dumping_profiles
#
# Unfortunately, at the moment AppArmor doesn't have a good story for backwards compatibility.
# https://askubuntu.com/questions/1517272/writing-a-backwards-compatible-apparmor-profile
if apparmor_status --enabled > /dev/null 2>&1; then
  APPARMOR_PROFILE_SOURCE='/opt/${sanitizedProductName}/resources/apparmor-profile'
  APPARMOR_PROFILE_TARGET='/etc/apparmor.d/${executable}'
  if apparmor_parser --skip-kernel-load --debug "$APPARMOR_PROFILE_SOURCE" > /dev/null 2>&1; then
    cp -f "$APPARMOR_PROFILE_SOURCE" "$APPARMOR_PROFILE_TARGET"

    # Updating the current AppArmor profile is not possible and probably not meaningful in a chroot'ed environment.
    # Use cases are for example environments where images for clients are maintained.
    # There, AppArmor might correctly be installed, but live updating makes no sense.
    if ! { [ -x '/usr/bin/ischroot' ] && /usr/bin/ischroot; } && hash apparmor_parser 2>/dev/null; then
      # Extra flags taken from dh_apparmor:
      # > By using '-W -T' we ensure that any abstraction updates are also pulled in.
      # https://wiki.debian.org/AppArmor/Contribute/FirstTimeProfileImport
      apparmor_parser --replace --write-cache --skip-read-cache "$APPARMOR_PROFILE_TARGET"
    fi
  else
    echo "Skipping the installation of the AppArmor profile as this version of AppArmor does not seem to support the bundled profile"
  fi
fi

# --- Wolkenfunk: APT repository ----------------------------------------------------------------
# Wolkenfunk addition to electron-builder's after-install.tpl above (keep that part in sync when
# electron-builder updates it). No ${...} below: electron-builder treats it as a template macro.
#
# Subscribes the system to updates: /etc/apt/sources.list.d/wolkenfunk.sources (deb822, signing
# key inline, so the file stands alone). Written on the first install, and refreshed on upgrades
# (new key, new URL) as long as it still carries our marker line. Whoever deletes the file or the
# marker keeps it that way. Removed on purge (after-remove.tpl).
WF_SOURCES=/etc/apt/sources.list.d/wolkenfunk.sources
WF_KEY='/opt/${sanitizedProductName}/resources/apt/wolkenfunk.asc'
WF_MARKER='# Managed by the wolkenfunk package'
if [ "$1" = configure ] && [ -f "$WF_KEY" ] && [ -d /etc/apt/sources.list.d ] && hash dpkg 2>/dev/null; then
  if { [ -z "$2" ] && [ ! -e "$WF_SOURCES" ]; } || grep -qF "$WF_MARKER" "$WF_SOURCES" 2>/dev/null; then
    {
      echo "$WF_MARKER (delete this line to manage the file yourself)."
      echo "Types: deb"
      echo "URIs: https://pkg.wolkenfunk.org/apt"
      echo "Suites: stable"
      echo "Components: main"
      echo "Architectures: $(dpkg --print-architecture)"
      echo "Signed-By:"
      sed -e 's/^$/./' -e 's/^/ /' "$WF_KEY"
    } > "$WF_SOURCES.tmp" && chmod 644 "$WF_SOURCES.tmp" && mv -f "$WF_SOURCES.tmp" "$WF_SOURCES"
  fi
fi
