#!/bin/bash
# Installiert das nctalk-Plugin in einem Agenten-Container und richtet den Kanal ein.
# Läuft auf dem Proxmox-Host als root.
#
#   NC_BASE_URL=https://cloud.example.org install-in-container.sh <CT-ID> <Nextcloud-Konto des Agenten> <Nextcloud-Konto des Besitzers> [Plugin-Archiv]
#
# Voraussetzungen:
#   - $AGENTS_DIR/<Agenten-Konto>.nc-app-password  (App-Passwort des Agenten-Kontos, chmod 600; Standard /root/agents)
#   - Plugin-Archiv (Standard: $AGENTS_DIR/openclaw-nctalk.tgz), erzeugt mit deploy/pack.sh
#   - Container: OpenClaw läuft als Benutzer "openclaw" (UID 1001) mit systemd-User-Dienst openclaw-gateway.service
# Das Skript ist wiederholbar: Ein zweiter Lauf aktualisiert Plugin und Einstellungen.
set -euo pipefail
CT="${1:?CT-ID fehlt}"; AGENT="${2:?Agenten-Konto fehlt}"; OWNER="${3:?Besitzer-Konto fehlt}"
AGENTS_DIR="${AGENTS_DIR:-/root/agents}"
ARCHIVE="${4:-$AGENTS_DIR/openclaw-nctalk.tgz}"
PWFILE="$AGENTS_DIR/${AGENT}.nc-app-password"
BASE_URL="${NC_BASE_URL:?NC_BASE_URL fehlt (Adresse der Nextcloud, z. B. https://cloud.example.org)}"
[ -s "$PWFILE" ] || { echo "App-Passwort fehlt: $PWFILE" >&2; exit 1; }
[ -s "$ARCHIVE" ] || { echo "Plugin-Archiv fehlt: $ARCHIVE" >&2; exit 1; }

echo "== Plugin nach CT $CT kopieren"
pct push "$CT" "$ARCHIVE" /tmp/openclaw-nctalk.tgz
pct exec "$CT" -- bash -c 'rm -rf /opt/openclaw-nctalk.new && mkdir -p /opt/openclaw-nctalk.new && tar -xzf /tmp/openclaw-nctalk.tgz -C /opt/openclaw-nctalk.new && rm -rf /opt/openclaw-nctalk.old && { [ -d /opt/openclaw-nctalk ] && mv /opt/openclaw-nctalk /opt/openclaw-nctalk.old || true; } && mv /opt/openclaw-nctalk.new /opt/openclaw-nctalk && chown -R root:root /opt/openclaw-nctalk && chmod -R a+rX /opt/openclaw-nctalk && rm -f /tmp/openclaw-nctalk.tgz'

echo "== App-Passwort hinterlegen"
pct exec "$CT" -- install -d -m 700 -o openclaw -g openclaw /home/openclaw/.openclaw/credentials
pct push "$CT" "$PWFILE" /home/openclaw/.openclaw/credentials/nctalk-app-password --perms 600 --user 1001 --group 1001

echo "== Konfiguration anpassen (Sicherung: openclaw.json.bak-pre-nctalk)"
pct exec "$CT" -- su - openclaw -c "AGENT='$AGENT' OWNER='$OWNER' BASE_URL='$BASE_URL' python3 - <<'PY'
import json, os, shutil
p = '/home/openclaw/.openclaw/openclaw.json'
if not os.path.exists(p + '.bak-pre-nctalk'):
    shutil.copy2(p, p + '.bak-pre-nctalk')
d = json.load(open(p))
plugins = d.setdefault('plugins', {})
paths = plugins.setdefault('load', {}).setdefault('paths', [])
if '/opt/openclaw-nctalk' not in paths:
    paths.append('/opt/openclaw-nctalk')
plugins.setdefault('entries', {}).setdefault('nctalk', {})['enabled'] = True
ch = d.setdefault('channels', {}).setdefault('nctalk', {})
ch.setdefault('enabled', True)
ch['baseUrl'] = os.environ['BASE_URL']
ch['user'] = os.environ['AGENT']
ch['passwordFile'] = '/home/openclaw/.openclaw/credentials/nctalk-app-password'
ch.setdefault('dmPolicy', 'allowlist')
allow = ch.setdefault('allowFrom', [])
if os.environ['OWNER'] not in allow:
    allow.append(os.environ['OWNER'])
ch.setdefault('groupPolicy', 'allowlist')
ch.setdefault('streaming', {'mode': 'progress'})
tmp = p + '.tmp-nctalk'
with open(tmp, 'w') as f:
    json.dump(d, f, indent=2, ensure_ascii=False)
    f.write('\n')
os.chmod(tmp, 0o600)
os.replace(tmp, p)
print('Kanal nctalk eingetragen fuer', os.environ['AGENT'], '- freigegeben:', allow)
PY"

echo "== Konfiguration prüfen"
pct exec "$CT" -- su - openclaw -c 'openclaw config validate' | tail -3

echo "== Gateway neu starten"
pct exec "$CT" -- su - openclaw -c 'export XDG_RUNTIME_DIR=/run/user/$(id -u); systemctl --user restart openclaw-gateway.service'
for i in $(seq 1 60); do
  sleep 2
  if pct exec "$CT" -- su - openclaw -c 'openclaw channels status --probe 2>/dev/null' | grep -q 'Nextcloud Talk (Benutzerkonto).*works'; then break; fi
done
pct exec "$CT" -- su - openclaw -c 'openclaw channels status --probe 2>/dev/null' | grep -E '^- ' || true
