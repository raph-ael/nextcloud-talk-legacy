# Nextcloud Talk Legacy

Schnelle, Signal-artige Clients für [Nextcloud Talk](https://nextcloud.com/talk/) –
Android, iOS, Windows, macOS und Linux. Offline-first, einfache Bedienung,
Chat und Anrufe über das Talk High-Performance-Backend.

> **Status:** frühe Entwicklung. Der Desktop-Client chattet bereits (Gesprächsliste,
> Live-Chat, Antworten, Lesebestätigungen, Benachrichtigungen); Android und iOS folgen.

**Ziel-MVP:** Login, Gesprächsliste, Text-Chats (1:1 und Gruppe), Audio-/Videoanrufe,
eingehende Anrufe klingeln auch bei geschlossener App.

## Architektur

Wie bei Signal: ein gemeinsamer **Rust-Kern**, darüber **native UIs** – keine
UI-Kompromisse, Protokoll- und Sync-Logik nur einmal geschrieben.

```
core/            Rust-Workspace
  talk-core/       Login Flow v2, OCS/Talk-API, (bald) Sync+DB, Signaling, Call-Zustandsmaschine, Push
  talk-ffi/        UniFFI → Kotlin (Android) / Swift (iOS)
  talk-node/       napi-rs → Electron (Desktop)
  talk-cli/        Test-CLI gegen einen echten Server
android/         Kotlin + Jetpack Compose
ios/             Swift (UIKit/SwiftUI), CallKit, PushKit – gebaut über Codemagic
desktop/         Electron + React + TypeScript
push-proxy/      Push-Proxy (FCM/APNs) für das Nextcloud-Push-v2-Protokoll
design/          Design-Tokens → Kotlin/Swift/CSS
```

Anforderungen an den Server: Nextcloud mit Talk ≥ 23; für Echtzeit-Chat und Anrufe
mit mehreren Teilnehmern der externe Signaling-Server (HPB, `nextcloud-spreed-signaling` ≥ 2.1).

## Entwicklung

Voraussetzungen: Rust (rustup), Node ≥ 24, JDK 21, Android SDK + NDK.

`.env` im Repo-Root (wird nicht eingecheckt):

```
NC_URL=https://cloud.example.org
NC_USER=…             NC_PASS=…           # eigener Account   (-a me)
BOT_A_USER=…          BOT_A_PASS=…        # Test-Account A    (-a a)
BOT_B_USER=…          BOT_B_PASS=…        # Test-Account B    (-a b)
TEST_ROOM_TOKEN=…                          # gemeinsamer Testraum
```

Als Passwort am besten ein App-Passwort verwenden (Einstellungen → Sicherheit).

```sh
cd core
cargo test --workspace
cargo run -p talk-cli -- -a a rooms
cargo run -p talk-cli -- -a a tail <token>          # folgt neuen Nachrichten
cargo run -p talk-cli -- -a b send <token> "Hallo"
cargo run -p talk-cli -- login cloud.example.org    # Login Flow v2 im Browser

# Bindings erzeugen
cargo build -p talk-ffi
cargo run -p talk-ffi --bin uniffi-bindgen -- generate \
  --library target/debug/libtalk_ffi.so --language kotlin --out-dir bindings/kotlin
cargo run -p talk-ffi --bin uniffi-bindgen -- generate \
  --library target/debug/libtalk_ffi.so --language swift --out-dir bindings/swift
```

### Desktop (Electron)

```sh
cd desktop
npm install
npm run core                 # Rust-Kern → native/talk_node.node
npm run dev                  # App mit Hot-Reload
# Entwicklung ohne Login, mit Test-Account aus ../.env:
TALK_DEV_ENV=../.env TALK_DEV_PREFIX=BOT_A npm run dev
npm run dist:linux           # AppImage + .deb
```

Unter Ubuntu ≥ 24.04 verweigert Electron im Dev-Modus ggf. den Start
(SUID-Sandbox/AppArmor): dann `npm run dev -- -- --no-sandbox`.

### Android

```sh
cd android
./gradlew :app:assembleRelease   # baut auch den Rust-Kern (cargo-ndk) und die Kotlin-Bindings
adb install -r app/build/outputs/apk/release/app-release.apk
```

Zum Ausprobieren und für Performance-Eindrücke immer den Release-Build nehmen (R8 + Baseline Profiles,
vorerst mit Debug-Schlüssel signiert). Debug-Builds laufen interpretiert und ruckeln beim ersten Scrollen
deutlich (gemessen: ~20 % verpasste Frames statt < 1 %).

Benötigt `cargo install cargo-ndk` und `rustup target add aarch64-linux-android x86_64-linux-android`.

## Server-Eigenheiten, die man kennen sollte

- **User-Agent und HTTP 426:** Talk akzeptiert Clients mit offiziellem Talk-User-Agent
  nur ab einer Mindestversion und schaltet Features daran frei. Wir melden uns als
  `Nextcloud-Talk v22.0.0` (mobil) bzw. `v1.0.0` (Desktop), siehe `TALK_COMPAT_*` in
  `core/talk-core/src/client.rs`. Auf Mobilgeräten ist der Talk-User-Agent nötig,
  damit Talk-Pushes an das Gerät gehen. Werden auf dem Server Ende-zu-Ende-verschlüsselte
  Anrufe aktiviert, steigt die Mindestversion – dann muss das Feature hier mit rein.
- Leere Objekte kommen als `[]` (PHP) – siehe tolerante Deserializer in `models.rs`.
- Ein Cookie-Jar pro Account: Talk bindet Raum-Sessions an die PHP-Session.
- Push: Der offizielle Nextcloud-Push-Proxy funktioniert nur mit den offiziellen Apps;
  eigene Builds brauchen einen eigenen Proxy (`push-proxy/`).

## Danksagung

Die Bedienkonzepte orientieren sich an den [Signal-Apps](https://github.com/signalapp).
Es wird kein Signal-Code übernommen.
