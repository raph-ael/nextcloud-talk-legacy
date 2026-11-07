# Sirplus Talk

Schnelle, Signal-artige Clients für Nextcloud Talk (cloud.sirplus.org) –
Android, iOS, Windows, macOS, Linux. Bundle-ID: `org.sirplus.talk`.

Planung: Deck-Board „Talk Clients 🚀“ – https://cloud.sirplus.org/apps/deck/board/17

## Architektur

Wie Signal: ein gemeinsamer **Rust-Kern**, darüber **native UIs**.

```
core/            Rust-Workspace
  talk-core/       Auth (Login Flow v2), OCS/Talk-API, (bald) Sync+DB, Signaling, Call-FSM, Push
  talk-ffi/        UniFFI → Kotlin (Android) / Swift (iOS)
  talk-node/       napi-rs → Electron (Desktop)
  talk-cli/        Test-CLI gegen den echten Server
android/         Kotlin + Jetpack Compose
ios/             Swift (UIKit/SwiftUI), CallKit, PushKit
desktop/         Electron + React + TypeScript
push-proxy/      Eigener Push-Proxy (FCM/APNs) für das Nextcloud-Push-v2-Protokoll
design/          Design-Tokens → Kotlin/Swift/CSS
```

## Entwicklung

Voraussetzungen: Rust (rustup), Node ≥ 24, JDK 21, Android SDK + NDK.

`.env` im Repo-Root (nicht eingecheckt):

```
NC_URL=https://cloud.sirplus.org
NC_USER=…            NC_PASS=…            # eigener Account (-a me)
BOT_A_USER=talk-bot-a BOT_A_PASS=…        # Test-Account (-a a)
BOT_B_USER=talk-bot-b BOT_B_PASS=…        # Test-Account (-a b)
TEST_ROOM_TOKEN=…                          # gemeinsamer Testraum
```

```sh
cd core
cargo test --workspace
cargo run -p talk-cli -- -a a rooms
cargo run -p talk-cli -- -a a tail <token>          # folgt neuen Nachrichten
cargo run -p talk-cli -- -a b send <token> "Hallo"
cargo run -p talk-cli -- login cloud.sirplus.org    # Login Flow v2 im Browser

# Bindings erzeugen
cargo build -p talk-ffi
cargo run -p talk-ffi --bin uniffi-bindgen -- generate \
  --library target/debug/libtalk_ffi.so --language kotlin --out-dir bindings/kotlin
cargo run -p talk-ffi --bin uniffi-bindgen -- generate \
  --library target/debug/libtalk_ffi.so --language swift --out-dir bindings/swift
```

## Wichtige Server-Eigenheiten

- **User-Agent / HTTP 426:** Talk akzeptiert Clients mit offiziellem Talk-User-Agent nur ab
  einer Mindestversion und schaltet Features daran frei. Wir melden uns als
  `Nextcloud-Talk v22.0.0` (mobil) bzw. `v1.0.0` (Desktop), siehe
  `TALK_COMPAT_*` in `core/talk-core/src/client.rs`. Der Talk-User-Agent ist auf
  Mobilgeräten nötig, damit Talk-Pushes an unser Gerät gehen.
- Leere Objekte kommen als `[]` (PHP) – siehe lenient Deserializer in `models.rs`.
- Ein Cookie-Jar pro Account: Talk bindet Raum-Sessions an die PHP-Session.

## Lizenz-Hinweis

Signal-Apps dienen nur als UX-Vorbild; es wird kein Signal-Code (AGPL) übernommen.
