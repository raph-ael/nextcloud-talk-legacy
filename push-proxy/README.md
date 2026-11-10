# Push-Proxy (Nextcloud Push v2 → FCM / APNs)

Nextclouds offizieller Push-Proxy bedient nur die offiziellen Nextcloud-Apps. Den Proxy
wählt aber der Client bei der Registrierung (`proxyServer`), deshalb kann jede eigene App
ihren eigenen betreiben. Dieser Dienst leitet die Ende-zu-Ende-verschlüsselten
Benachrichtigungen an Firebase Cloud Messaging (Android) und Apple Push (iOS, VoIP über
PushKit) weiter. Lesen kann er sie nicht: Der Server verschlüsselt jede Benachrichtigung
mit dem Schlüssel des Geräts.

## Endpunkte

| Methode | Pfad | von | Inhalt |
|---|---|---|---|
| `POST` | `/devices` | App | `pushToken`, `deviceIdentifier`, `deviceIdentifierSignature`, `userPublicKey` |
| `DELETE` | `/devices` | App | `deviceIdentifier`, `deviceIdentifierSignature`, `userPublicKey` |
| `POST` | `/notifications` | Nextcloud | `notifications[]` (JSON) → `{"unknown": [...], "failed": n}` |
| `GET` | `/health` | Monitoring | `ok` |

**Regeln:**
- Jede Signatur wird geprüft (RSA-SHA512 mit dem Benutzer-Schlüssel des Servers).
- Pushes für Geräte, die FCM/APNs nicht mehr kennen, meldet der Proxy als `unknown`. Der Server vergisst sie dann.
- **iOS-Tokens** haben die Form `"<APNs-Token> <PushKit-Token>"`:
  - `voip` geht an PushKit (Topic `<bundle>.voip`).
  - `background` geht an APNs.
  - `alert` nur mit `APNS_FORWARD_ALERTS=1`, dafür braucht die App eine Notification Service Extension.

## Konfiguration (Umgebung)

| Variable | Bedeutung |
|---|---|
| `PUSH_LISTEN` | Adresse, Standard `127.0.0.1:8090` (hinter einem Reverse-Proxy mit TLS betreiben) |
| `PUSH_DB` | SQLite-Datei, Standard `push-proxy.sqlite` |
| `FCM_SERVICE_ACCOUNT` | Pfad zum Firebase-Service-Account-JSON (Rolle „Firebase Cloud Messaging API Admin“) |
| `APNS_KEY` | Pfad zum APNs-Schlüssel (`.p8`) |
| `APNS_KEY_ID`, `APNS_TEAM_ID` | Key-ID und Team-ID aus dem Apple-Developer-Konto |
| `APNS_TOPIC` | Bundle-ID der App |
| `APNS_FORWARD_ALERTS` | `1`: auch normale Benachrichtigungen an iOS |
| `RUST_LOG` | z. B. `talk_push_proxy=debug` |

Development-Builds von iOS haben Sandbox-Tokens. Der Proxy probiert erst Production, dann Sandbox, und merkt sich das pro Gerät.

## Betrieb

```sh
cargo build --release
install -m 755 target/release/talk-push-proxy /usr/local/bin/
cp talk-push-proxy.service /etc/systemd/system/ && systemctl enable --now talk-push-proxy
```

Die Unit liest die Umgebung aus `/etc/talk-push-proxy.env`. Schlüsseldateien bekommen nur
die Rechte `0600` und gehören dem Dienstbenutzer. Davor gehört ein Reverse-Proxy mit TLS,
z. B. nginx `location / { proxy_pass http://127.0.0.1:8090; }`.
