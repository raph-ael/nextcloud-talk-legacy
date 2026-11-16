//! Shape checks with the clients' own wire types (talk-core), and an end-to-end run of
//! talk-core's client and sync engine against the server on a free local port.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::Value;
use talk_core::{
    Account, ChatMessage, ConnectionState, ConversationTag, Engine, Event, EventSink, MessageOp, MessageView, Participant,
    Platform, PushMessage, Reactions, Room, TalkClient, message_error_text,
};

use crate::state::State;

fn fixtures() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures"))
}

pub fn state() -> State {
    State::load(fixtures()).expect("fixtures")
}

#[test]
fn rooms_parse_as_talk_rooms() {
    let st = state();
    let json: Vec<Value> = st.rooms.iter().map(|r| st.room_json(r)).collect();
    let rooms: Vec<Room> = serde_json::from_value(Value::Array(json)).unwrap();
    assert!(rooms.len() >= 8);
    let garden = rooms.iter().find(|r| r.token == "garten").unwrap();
    assert_eq!(garden.unread_messages, 5);
    assert!(garden.last_read_message < garden.last_message.as_ref().unwrap().id);
    let website = rooms.iter().find(|r| r.token == "website").unwrap();
    assert!(website.unread_mention);
    assert!(rooms.iter().any(|r| r.is_favorite));
    assert!(rooms.iter().filter(|r| r.unread_messages > 0).count() >= 3);
    let lena = rooms.iter().find(|r| r.token == "lena").unwrap();
    assert_eq!(lena.display_name, "Lena Hoffmann");
    assert_eq!(lena.unread_messages, 0);
    assert!(!rooms.iter().find(|r| r.token == "familie").unwrap().tag_ids.is_empty());
}

#[test]
fn messages_parse_with_attachments_replies_reactions() {
    let st = state();
    let mut all = Vec::new();
    for r in &st.rooms {
        let json: Vec<Value> = r.messages.iter().map(|m| st.msg_json(r, m, true)).collect();
        let msgs: Vec<ChatMessage> = serde_json::from_value(Value::Array(json)).unwrap();
        // Ascending ids and timestamps per conversation.
        assert!(msgs.windows(2).all(|w| w[0].id < w[1].id && w[0].timestamp <= w[1].timestamp));
        all.extend(msgs);
    }
    let image = all.iter().find_map(|m| m.attachment().filter(|a| a.is_image())).unwrap();
    assert!(image.preview_available && image.width > 0 && image.size > 0);
    let voice = all.iter().find_map(|m| m.attachment().filter(|a| a.voice_message)).unwrap();
    assert_eq!(voice.mimetype, "audio/mpeg");
    assert!(all.iter().any(|m| m.parent.is_some()));
    assert!(all.iter().any(|m| m.reactions.get("👍").is_some_and(|n| *n >= 2)));
    assert!(all.iter().any(|m| m.reactions_self.contains(&"❤️".to_string())));
    assert!(all.iter().any(|m| m.system_message == "conversation_created"));
    let mention = all.iter().find(|m| m.message.contains("{mention-user1}")).unwrap();
    assert!(mention.rendered_text().starts_with("@Mira Lange"));
}

#[test]
fn reactions_tags_participants_parse() {
    let st = state();
    let room = st.rooms.iter().find(|r| r.token == "familie").unwrap();
    let liked = room.messages.iter().find(|m| m.reactions.len() >= 3).unwrap();
    let r: Reactions = serde_json::from_value(st.reactions_json(liked, None)).unwrap();
    assert_eq!(r.0["❤️"].len(), 3);
    let none: Reactions = serde_json::from_value(st.reactions_json(&room.messages[0], None)).unwrap();
    assert!(none.0.is_empty());
    let tags: Vec<ConversationTag> =
        serde_json::from_value(Value::Array(st.tags.iter().map(|t| t.json()).collect())).unwrap();
    assert!(tags.iter().any(|t| t.kind == "favorites") && tags.iter().any(|t| t.kind == "other"));
    assert!(tags.iter().any(|t| t.name == "Arbeit"));
    let members: Vec<Participant> = serde_json::from_value(st.participants_json(room)).unwrap();
    assert_eq!(members.len(), 4);
}

struct Sink(Mutex<Vec<Event>>);

impl EventSink for Sink {
    fn emit(&self, event: Event) {
        self.0.lock().unwrap().push(event);
    }
}

async fn serve() -> String {
    serve_from(fixtures()).await
}

async fn serve_from(dir: &Path) -> String {
    let app = crate::app_from(dir).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, crate::router(app)).await.unwrap() });
    format!("http://{addr}")
}

fn client(server: &str) -> TalkClient {
    let account = Account { server: server.into(), user: "demo".into(), app_password: "demo".into() };
    TalkClient::new(account, Platform::Cli).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn talk_core_client_end_to_end() {
    let server = serve().await;
    let c = client(&server);
    assert_eq!(c.current_user().await.unwrap().id, "mira");
    assert!(c.get_app_password().await.unwrap().starts_with("demo-"));
    let caps = c.capabilities().await.unwrap();
    assert!(caps.pointer("/capabilities/notify_push").is_none());
    assert_eq!(c.attachment_folder().await.unwrap(), "/Talk");

    let batch = c.rooms(None, false).await.unwrap();
    assert!(batch.modified_before.is_some() && batch.talk_hash.is_some());
    let since = batch.modified_before.unwrap();
    assert!(c.rooms(Some(since + 10), true).await.unwrap().rooms.is_empty());

    let hist = c.history("garten", None, 50).await.unwrap();
    assert!(hist.messages.len() > 5 && hist.last_common_read.is_some());
    let newest = hist.messages.last().unwrap().id;
    // Nothing new: the long poll ends after its timeout with 304 (empty batch).
    let t = std::time::Instant::now();
    assert!(c.poll_messages("garten", newest, 1).await.unwrap().messages.is_empty());
    assert!(t.elapsed() >= Duration::from_millis(900));

    // A waiting poll gets a new message right away.
    let c2 = c.clone();
    let poll = tokio::spawn(async move { c2.poll_messages("garten", newest, 30).await });
    tokio::time::sleep(Duration::from_millis(200)).await;
    let reference = talk_core::new_reference_id();
    let sent = c.send_message("garten", "Ich gieße am Samstag! 💧", &reference, Some(newest)).await.unwrap();
    assert_eq!(sent.reference_id, reference);
    assert_eq!(sent.parent.as_ref().unwrap().id, newest);
    let got = tokio::time::timeout(Duration::from_secs(3), poll).await.unwrap().unwrap().unwrap();
    assert_eq!(got.messages[0].id, sent.id);
    assert_eq!(got.last_given, Some(sent.id));

    // Reactions: add, list, remove; the hidden system message carries the update.
    let r = c.add_reaction("garten", newest, "👍").await.unwrap();
    assert_eq!(r.0["👍"][0].actor_id, "mira");
    assert_eq!(c.reactions("garten", newest).await.unwrap().0.len(), 1);
    let poll = c.poll_messages("garten", sent.id, 0).await.unwrap();
    assert_eq!(poll.messages[0].system_message, "reaction");
    assert_eq!(poll.messages[0].parent.as_ref().unwrap().reactions_self, vec!["👍".to_string()]);
    assert!(c.remove_reaction("garten", newest, "👍").await.unwrap().0.is_empty());
    assert!(c.remove_reaction("garten", newest, "👍").await.is_err());

    c.set_read_marker("garten", sent.id).await.unwrap();
    assert_eq!(c.room("garten").await.unwrap().unread_messages, 0);
    assert!(c.join_room("garten", false).await.unwrap().starts_with("demo-session-"));
    c.leave_room("garten").await.unwrap();
    assert_eq!(c.members("garten").await.unwrap().len(), 4);
    assert!(c.tags().await.unwrap().len() >= 4);
    assert!(c.search_users("len", 10).await.unwrap().iter().any(|u| u.id == "lena"));

    // Pictures are PNG/JPEG (Android cannot decode SVG).
    let (png, mime) = c.room_avatar("familie", true).await.unwrap();
    assert!(png.starts_with(b"\x89PNG") && mime == "image/png");
    let (png, _) = c.user_avatar("lena", false).await.unwrap();
    assert!(png.starts_with(b"\x89PNG"));
    let image = hist.messages.iter().find_map(|m| m.attachment().filter(|a| a.is_image())).unwrap();
    let jpeg = c.file_preview(&image.id, 400, 400).await.unwrap();
    assert!(jpeg.starts_with(&[0xff, 0xd8]));
    let resp = c.download("mira", &format!("/{}", image.path), 100).await.unwrap();
    assert_eq!(resp.status().as_u16(), 206);
    assert_eq!(resp.bytes().await.unwrap().len() as i64, image.size - 100);

    // Categories: create one and put a chat in it.
    let tag = c.create_tag("Hobbys").await.unwrap();
    let room = c.set_room_tags("lesekreis", std::slice::from_ref(&tag.id)).await.unwrap();
    assert_eq!(room.tag_ids, vec![tag.id.clone()]);

    // Unknown endpoints answer with an empty success.
    assert!(c.notifications().await.unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn engine_and_login_flow() {
    let server = serve().await;
    let flow = talk_core::LoginFlow::start(&server, Platform::Cli).await.unwrap();
    assert!(flow.poll_once().await.unwrap().is_none());
    // Opening the page in the browser approves the login.
    let page = reqwest::get(&flow.login_url).await.unwrap();
    assert!(page.status().is_success());
    let creds = flow.poll_once().await.unwrap().unwrap();
    assert_eq!(creds.server, server);

    let dir = std::env::temp_dir().join(format!("talk-demo-test-{}", uuid::Uuid::new_v4().simple()));
    let sink = Arc::new(Sink(Mutex::new(Vec::new())));
    let account = Account { server: creds.server, user: creds.login_name, app_password: creds.app_password };
    let engine = Engine::new(TalkClient::new(account, Platform::Cli).unwrap(), sink.clone(), Some(dir.clone()));
    engine.start().await.unwrap();
    assert_eq!(engine.user_id(), "mira");
    let rooms = engine.rooms();
    assert!(rooms[0].is_favorite, "favorite first");
    assert!(rooms[1].last_activity >= rooms[2].last_activity);
    let garden = rooms.iter().find(|r| r.token == "garten").unwrap();
    assert_eq!(garden.unread_messages, 5);
    assert!(garden.can_react);
    let titles: Vec<String> = engine.room_sections("", None).into_iter().map(|s| s.title).collect();
    assert_eq!(titles, vec!["Favoriten", "Familie & Freunde", "Arbeit", "Chats"]);

    // Chat menu (docs/settings.md): the default level is resolved, actions show at once.
    let room = |token: &str| engine.rooms().into_iter().find(|r| r.token == token).unwrap();
    assert_eq!(room("lena").notification_level, 1);
    assert_eq!(room("lesekreis").notification_level, 2);
    assert_eq!(room("sommerfest").notification_level, 3);
    engine.set_favorite("lena", true).await.unwrap();
    assert!(room("lena").is_favorite);
    assert!(engine.rooms()[..2].iter().any(|r| r.token == "lena"), "favorites sort first");
    engine.set_favorite("lena", false).await.unwrap();
    assert!(!room("lena").is_favorite);
    assert!(engine.set_favorite("gibt-es-nicht", true).await.is_err());
    engine.set_room_categories("lesekreis", &["3".into(), "4".into()]).await.unwrap();
    assert_eq!(room("lesekreis").tag_ids, vec!["3", "4"]);
    let sections = engine.room_sections("", None);
    assert!(sections.iter().filter(|s| s.rooms.iter().any(|r| r.token == "lesekreis")).count() == 2);
    engine.set_room_categories("lesekreis", &[]).await.unwrap();
    assert!(room("lesekreis").tag_ids.is_empty());
    engine.mark_room_read("website").await.unwrap();
    assert_eq!(room("website").unread_messages, 0);
    assert!(!room("website").unread_mention);

    let page = engine.open_chat("familie").await.unwrap();
    assert!(page.messages.iter().any(|m| m.kind == "system"));
    assert!(page.messages.iter().any(|m| m.is_own));
    let voice = page.messages.iter().find_map(|m| m.attachment.clone().filter(|a| a.voice_message)).unwrap();
    let local = engine.fetch_file(&voice.path, voice.size).await.unwrap();
    let info = engine.audio_info(&local).await.unwrap();
    assert!(info.duration_ms > 3000, "voice message decodes ({} ms)", info.duration_ms);
    engine.shutdown();
    let _ = std::fs::remove_dir_all(dir);
}

/// Two accounts on two servers (docs/accounts.md): they are told apart, and a message
/// for one of them shows in its badge only, by the rules for muted chats.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_accounts_and_their_badges() {
    let a = serve().await;
    let b = serve_from(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures-b"))).await;

    let who_a = talk_core::resolve_account(&a, "demo", "demo", Platform::Cli).await.unwrap();
    let who_b = talk_core::resolve_account(&b, "demo", "demo", Platform::Cli).await.unwrap();
    assert_eq!((who_a.user_id.as_str(), who_b.user_id.as_str()), ("mira", "jonas"));
    assert_eq!(who_b.display_name, "Jonas Richter");
    assert_ne!(who_a.key, who_b.key);
    // The same login again is the same account, however the address was typed.
    let again = talk_core::resolve_account(&format!("{}/", b.to_uppercase().replace("HTTP", "http")), "x", "y", Platform::Cli)
        .await
        .unwrap();
    assert_eq!(again.key, who_b.key);
    talk_core::revoke_app_password(&b, "demo", "demo", Platform::Cli).await.unwrap();

    let dir = std::env::temp_dir().join(format!("talk-demo-test-{}", uuid::Uuid::new_v4().simple()));
    let engine = |server: &str, name: &str| {
        let account = Account { server: server.into(), user: "demo".into(), app_password: "demo".into() };
        let sink = Arc::new(Sink(Mutex::new(Vec::new())));
        Engine::new(TalkClient::new(account, Platform::Cli).unwrap(), sink, Some(dir.join(name)))
    };
    let (ea, eb) = (engine(&a, "a"), engine(&b, "b"));
    // Before the server answered, the login name stands in for the name.
    assert_eq!(eb.display_name(), "demo");
    ea.start().await.unwrap();
    eb.start().await.unwrap();
    assert_eq!(eb.display_name(), "Jonas Richter");

    // Account A: only the group with a mention counts, the other unread groups are at
    // "Nur Erwähnungen" by default.
    let badge_a = ea.badge();
    assert_eq!((badge_a.chats, badge_a.messages), (1, 2));
    assert_eq!(eb.badge(), talk_core::BadgeView::default());

    let incoming = |token: &'static str, text: &'static str| {
        let url = format!("{b}/demo/incoming");
        async move {
            let r = reqwest::Client::new().post(url).form(&[("token", token), ("text", text)]).send().await.unwrap();
            assert_eq!(r.status(), 201, "{token}");
        }
    };
    // "Nur Erwähnungen": a plain message does not count, a mention does.
    incoming("werkstatt", "Wer kommt am Donnerstag?").await;
    eb.refresh_rooms(true).await.unwrap();
    assert_eq!(eb.badge().chats, 0);
    incoming("werkstatt", "@jonas bringst du den Ständer mit?").await;
    // One-to-one and "Alle Nachrichten" always count.
    incoming("mira", "Bist du schon da?").await;
    incoming("garten", "Die Leiter steht am Tor.").await;
    eb.refresh_rooms(true).await.unwrap();
    let badge_b = eb.badge();
    assert_eq!((badge_b.chats, badge_b.messages), (3, 4));
    // Nothing of it reaches the other account, although both have a chat "garten".
    ea.refresh_rooms(true).await.unwrap();
    assert_eq!(ea.badge(), badge_a);
    assert_ne!(
        ea.rooms().iter().find(|r| r.token == "garten").unwrap().name,
        eb.rooms().iter().find(|r| r.token == "garten").unwrap().name
    );

    // The name is known at once the next time, from the cache.
    eb.shutdown();
    drop(eb);
    assert_eq!(engine(&b, "b").display_name(), "Jonas Richter");

    // Only members can write, and not the user themselves.
    let bad = reqwest::Client::new()
        .post(format!("{b}/demo/incoming"))
        .form(&[("token", "mira"), ("text", "x"), ("from", "jonas")])
        .send()
        .await
        .unwrap();
    assert_eq!(bad.status(), 400);
    ea.shutdown();
    let _ = std::fs::remove_dir_all(dir);
}

/// What the engine announced since the last call: `(tokens with a notice, since_start)`,
/// cleared tokens and missed calls.
fn announced(sink: &Sink) -> (Vec<String>, bool, Vec<String>, Vec<String>) {
    let mut notices = Vec::new();
    let mut since_start = false;
    let mut cleared = Vec::new();
    let mut missed = Vec::new();
    for e in sink.0.lock().unwrap().drain(..) {
        match e {
            Event::MessageNotices { items, since_start: s } => {
                since_start |= s;
                notices.extend(items.into_iter().map(|n| n.token));
            }
            Event::NoticesCleared { tokens } => cleared.extend(tokens),
            Event::CallMissed { token, caller_name, .. } => missed.push(format!("{token}:{caller_name}")),
            _ => {}
        }
    }
    (notices, since_start, cleared, missed)
}

/// docs/notifications.md end to end: new messages are announced once, by the rules for
/// muted chats and "do not disturb", cleared when read, reported after a restart, and a
/// call missed while the app was away is reported once.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn notices_for_new_messages_and_missed_calls() {
    let server = serve_from(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures-b"))).await;
    let dir = std::env::temp_dir().join(format!("talk-demo-test-{}", uuid::Uuid::new_v4().simple()));
    let open = || {
        let account = Account { server: server.clone(), user: "demo".into(), app_password: "demo".into() };
        let sink = Arc::new(Sink(Mutex::new(Vec::new())));
        (Engine::new(TalkClient::new(account, Platform::Cli).unwrap(), sink.clone(), Some(dir.clone())), sink)
    };
    let post = |path: &'static str, form: Vec<(&'static str, &'static str)>| {
        let url = format!("{server}{path}");
        async move {
            let r = reqwest::Client::new().post(url).form(&form).send().await.unwrap();
            assert!(r.status().is_success(), "{path}: {}", r.status());
        }
    };
    let none: Vec<String> = Vec::new();

    let (engine, sink) = open();
    engine.start().await.unwrap();
    // The look at the notifications that `start` spawned sets the starting point.
    engine.check_incoming_calls().await;
    // Signing in is a starting point: nothing is announced.
    assert_eq!(announced(&sink), (none.clone(), false, none.clone(), none.clone()));

    // A message in a one-to-one chat: announced once, with the text.
    post("/demo/incoming", vec![("token", "mira"), ("text", "Bist du schon da?")]).await;
    engine.refresh_rooms(false).await.unwrap();
    let notice = sink.0.lock().unwrap().iter().find_map(|e| match e {
        Event::MessageNotices { items, .. } => items.first().cloned(),
        _ => None,
    });
    let notice = notice.expect("no notice for the new message");
    assert_eq!((notice.room_name.as_str(), notice.is_group, notice.unread, notice.mention), ("Mira Lange", false, 1, false));
    assert_eq!(notice.message.as_ref().map(|m| m.text.as_str()), Some("Bist du schon da?"));
    assert_eq!(announced(&sink), (vec!["mira".to_string()], false, none.clone(), none.clone()));
    engine.refresh_rooms(true).await.unwrap();
    assert_eq!(announced(&sink).0, none);

    // "Nur Erwähnungen": only the mention announces.
    post("/demo/incoming", vec![("token", "werkstatt"), ("text", "Wer kommt?")]).await;
    engine.refresh_rooms(false).await.unwrap();
    assert_eq!(announced(&sink).0, none);
    post("/demo/incoming", vec![("token", "werkstatt"), ("text", "@jonas bringst du den Ständer mit?")]).await;
    engine.refresh_rooms(false).await.unwrap();
    assert_eq!(announced(&sink).0, ["werkstatt"]);

    // Read here: the notification is obsolete.
    engine.mark_room_read("mira").await.unwrap();
    assert_eq!(announced(&sink).2, ["mira"]);

    // "Do not disturb": nothing, and what was held back does not come later.
    post("/demo/status", vec![("status", "dnd")]).await;
    engine.refresh_rooms(true).await.unwrap();
    post("/demo/incoming", vec![("token", "garten"), ("text", "Die Leiter steht am Tor.")]).await;
    engine.refresh_rooms(false).await.unwrap();
    post("/demo/missed-call", vec![("token", "clara")]).await;
    engine.check_incoming_calls().await;
    post("/demo/status", vec![("status", "online")]).await;
    engine.refresh_rooms(true).await.unwrap();
    engine.check_incoming_calls().await;
    assert_eq!(announced(&sink), (none.clone(), false, none.clone(), none.clone()));

    // A call missed while the app was not looking: reported once. An old one is not.
    post("/demo/missed-call", vec![("token", "mira")]).await;
    post("/demo/missed-call", vec![("token", "clara"), ("ago", "90000")]).await;
    engine.check_incoming_calls().await;
    engine.check_incoming_calls().await;
    assert_eq!(announced(&sink).3, ["mira:Mira Lange"]);

    // Closed, a message comes in, started again: announced as "since the start".
    engine.shutdown();
    drop(engine);
    tokio::time::sleep(Duration::from_millis(300)).await;
    post("/demo/incoming", vec![("token", "clara"), ("text", "Bis Samstag!")]).await;
    let (engine, sink) = open();
    engine.start().await.unwrap();
    engine.check_incoming_calls().await;
    let (notices, since_start, _, missed) = announced(&sink);
    assert_eq!((notices, since_start), (vec!["clara".to_string()], true));
    assert_eq!(missed, none, "a missed call was reported again after the restart");
    engine.shutdown();
    let _ = std::fs::remove_dir_all(dir);
}

/// "Als gelesen markieren" in a notification can start the app: the cached list is then
/// older than the message the notification shows. The marker must still reach the server
/// and the notification must be cleared.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mark_read_from_a_notification_after_a_cold_start() {
    let server = serve_from(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures-b"))).await;
    let dir = std::env::temp_dir().join(format!("talk-demo-test-{}", uuid::Uuid::new_v4().simple()));
    let account = || Account { server: server.clone(), user: "demo".into(), app_password: "demo".into() };
    let open = || {
        let sink = Arc::new(Sink(Mutex::new(Vec::new())));
        (Engine::new(TalkClient::new(account(), Platform::Cli).unwrap(), sink.clone(), Some(dir.clone())), sink)
    };
    // A first run fills the cache; then the app is closed.
    let (engine, _) = open();
    engine.start().await.unwrap();
    engine.shutdown();
    drop(engine);
    tokio::time::sleep(Duration::from_millis(300)).await;

    // A message comes in (the push shows it), and the action starts the app: the engine
    // exists, but has not asked the server for anything yet.
    let r = reqwest::Client::new()
        .post(format!("{server}/demo/incoming"))
        .form(&[("token", "mira"), ("text", "Bist du schon da?")])
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 201);
    let (engine, sink) = open();
    assert_eq!(engine.rooms().iter().find(|r| r.token == "mira").unwrap().unread_messages, 0, "the cache knows nothing yet");
    engine.mark_room_read("mira").await.unwrap();
    assert_eq!(announced(&sink).2, ["mira"]);

    // The server agrees: nothing unread any more.
    let rooms = TalkClient::new(account(), Platform::Cli).unwrap().rooms(None, true).await.unwrap().rooms;
    assert_eq!(rooms.iter().find(|r| r.token == "mira").unwrap().unread_messages, 0);
    let _ = std::fs::remove_dir_all(dir);
}

/// A demo server that can go away and come back at the same address with what it knew.
struct Outage {
    app: crate::App,
    addr: std::net::SocketAddr,
    running: Option<(tokio::sync::oneshot::Sender<()>, tokio::task::JoinHandle<()>)>,
}

impl Outage {
    async fn server() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut s = Self { app: crate::app_from(fixtures()).unwrap(), addr: listener.local_addr().unwrap(), running: None };
        s.serve(listener);
        s
    }

    fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    fn serve(&mut self, listener: tokio::net::TcpListener) {
        let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
        let router = crate::router(self.app.clone());
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, router).with_graceful_shutdown(async { let _ = stopped.await; }).await;
        });
        self.running = Some((stop, task));
    }

    /// Closes the port and the idle connections: every request fails from now on.
    async fn stop(&mut self) {
        let (stop, task) = self.running.take().unwrap();
        let _ = stop.send(());
        tokio::time::timeout(Duration::from_secs(5), task).await.expect("server stops").unwrap();
    }

    async fn start(&mut self) {
        let listener = tokio::net::TcpListener::bind(self.addr).await.unwrap();
        self.serve(listener);
    }

    fn engine(&self) -> (Arc<Engine>, Arc<Sink>) {
        let account = Account { server: self.url(), user: "demo".into(), app_password: "demo".into() };
        let sink = Arc::new(Sink(Mutex::new(Vec::new())));
        (Engine::new(TalkClient::new(account, Platform::Cli).unwrap(), sink.clone(), None), sink)
    }
}

/// The connection states the platform was told, in order.
fn connections(sink: &Sink) -> Vec<ConnectionState> {
    sink.0.lock().unwrap().iter().filter_map(|e| match e { Event::Connection { state } => Some(*state), _ => None }).collect()
}

async fn until(what: &str, cond: impl Fn() -> bool) {
    for _ in 0..100 {
        if cond() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("timed out: {what}");
}

/// docs/connection.md: one failed request shows as `connecting` and is retried at once;
/// it never shows as an outage.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_single_failure_is_no_outage() {
    use ConnectionState::*;
    let mut server = Outage::server().await;
    let (engine, sink) = server.engine();
    assert_eq!(engine.connection_state(), Connecting, "a session starts as connecting");
    engine.start().await.unwrap();
    // The list is there before the state says online: an empty list while connecting is
    // "still loading".
    let events = sink.0.lock().unwrap();
    let rooms = events.iter().position(|e| matches!(e, Event::RoomsChanged { .. })).unwrap();
    let online = events.iter().position(|e| matches!(e, Event::Connection { state: Online })).unwrap();
    assert!(rooms < online, "rooms before online");
    drop(events);
    assert_eq!(connections(&sink), [Online]);

    server.stop().await;
    assert!(engine.refresh_rooms(false).await.is_err());
    assert_eq!(engine.connection_state(), Connecting);
    server.start().await;
    // The retry at once, not the regular refresh after ten seconds.
    let since = std::time::Instant::now();
    until("online again", || engine.connection_state() == Online).await;
    assert!(since.elapsed() < Duration::from_secs(1), "took {:?}", since.elapsed());
    assert_eq!(connections(&sink), [Online, Connecting, Online]);
    engine.shutdown();
}

/// The platform says there is no network: `connecting` without waiting for a request to
/// fail, `offline` when nothing answers, and no outage when the server answers after all.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_platform_reports_no_network() {
    use ConnectionState::*;
    let mut server = Outage::server().await;
    let (engine, sink) = server.engine();
    engine.start().await.unwrap();

    // A false alarm: the first retry is answered before the platform would show anything.
    engine.network_lost();
    assert_eq!(engine.connection_state(), Connecting);
    let since = std::time::Instant::now();
    until("online again", || engine.connection_state() == Online).await;
    assert!(since.elapsed() < Duration::from_millis(500), "took {:?}", since.elapsed());

    server.stop().await;
    engine.network_lost();
    assert_eq!(engine.connection_state(), Connecting);
    let since = std::time::Instant::now();
    until("offline", || engine.connection_state() == Offline).await;
    assert!(since.elapsed() > Duration::from_secs(2), "took {:?}", since.elapsed());
    assert!(since.elapsed() < Duration::from_secs(5), "took {:?}", since.elapsed());
    engine.network_lost();
    assert_eq!(connections(&sink), [Online, Connecting, Online, Connecting, Offline]);

    server.start().await;
    engine.network_changed();
    until("online", || engine.connection_state() == Online).await;
    engine.shutdown();
}

/// The server stays away: `offline` when the retries keep failing, a message written meanwhile
/// waits, and both recover when the platform reports the network.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_outage_and_its_end() {
    use ConnectionState::*;
    let mut server = Outage::server().await;
    let (engine, sink) = server.engine();
    engine.start().await.unwrap();
    server.stop().await;
    assert!(engine.refresh_rooms(false).await.is_err());
    let since = std::time::Instant::now();
    until("offline", || engine.connection_state() == Offline).await;
    // Long enough for a change of network (retries after 0.1 s, 0.6 s, 1.6 s and 3.6 s).
    assert!(since.elapsed() > Duration::from_secs(3), "took {:?}", since.elapsed());
    assert_eq!(connections(&sink), [Online, Connecting, Offline]);

    let pending = engine.enqueue_text("lena", "Bis gleich!", None, "ref-outage");
    until("waiting", || engine.pending("lena").iter().any(|p| p.state == talk_core::PendingState::Waiting)).await;
    assert_eq!(pending.message.text, "Bis gleich!");
    // More failures change nothing, and a network report without a server does not flicker.
    assert!(engine.refresh_rooms(false).await.is_err());
    engine.network_changed();
    tokio::time::sleep(Duration::from_millis(700)).await;
    assert_eq!(connections(&sink), [Online, Connecting, Offline]);

    server.start().await;
    engine.network_changed();
    let since = std::time::Instant::now();
    until("online again", || engine.connection_state() == Online).await;
    assert!(since.elapsed() < Duration::from_secs(2), "took {:?}", since.elapsed());
    until("message sent", || engine.outbox_unsent() == 0).await;
    assert_eq!(connections(&sink), [Online, Connecting, Offline, Online]);

    // Without a word from the platform the retries find the server themselves, sooner
    // than the regular refresh would.
    server.stop().await;
    assert!(engine.refresh_rooms(false).await.is_err());
    until("offline", || engine.connection_state() == Offline).await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    server.start().await;
    let since = std::time::Instant::now();
    until("online by itself", || engine.connection_state() == Online).await;
    assert!(since.elapsed() < Duration::from_secs(6), "took {:?}", since.elapsed());
    engine.shutdown();
}

/// A first start without a server: `connecting`, then `offline`, never "no chats" as a
/// fact; the list arrives when the server does.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_start_without_the_server() {
    use ConnectionState::*;
    let mut server = Outage::server().await;
    server.stop().await;
    let (engine, sink) = server.engine();
    assert!(engine.start().await.is_err());
    assert!(engine.rooms().is_empty());
    assert_eq!(engine.connection_state(), Connecting);
    until("offline", || engine.connection_state() == Offline).await;
    assert_eq!(connections(&sink), [Offline]);

    server.start().await;
    // Another call is answered first: no `online` without the list.
    engine.search_users("le", None).await.unwrap();
    assert!(engine.connection_state() != Online || !engine.rooms().is_empty());
    engine.network_changed();
    until("online", || engine.connection_state() == Online).await;
    assert!(!engine.rooms().is_empty());
    assert_eq!(connections(&sink), [Offline, Online]);
    let events = sink.0.lock().unwrap();
    let rooms = events.iter().position(|e| matches!(e, Event::RoomsChanged { .. })).unwrap();
    let online = events.iter().position(|e| matches!(e, Event::Connection { state: Online })).unwrap();
    assert!(rooms < online, "rooms before online");
    drop(events);
    engine.shutdown();
}

/// A server that takes the connection and never answers: the first requests only fail
/// after their timeout, the state says `offline` long before.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_start_with_a_server_that_hangs() {
    use ConnectionState::*;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let server = format!("http://{}", listener.local_addr().unwrap());
    let hanging = tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((stream, _)) = listener.accept().await {
            held.push(stream);
        }
    });
    let sink = Arc::new(Sink(Mutex::new(Vec::new())));
    let account = Account { server, user: "demo".into(), app_password: "demo".into() };
    let engine = Engine::new(TalkClient::new(account, Platform::Cli).unwrap(), sink.clone(), None);
    let starting = tokio::spawn({
        let engine = engine.clone();
        async move { engine.start().await }
    });
    tokio::time::sleep(Duration::from_secs(6)).await;
    assert_eq!(engine.connection_state(), Connecting);
    assert!(connections(&sink).is_empty());
    until("offline", || engine.connection_state() == Offline).await;
    assert_eq!(connections(&sink), [Offline]);
    assert!(!starting.is_finished(), "the requests still hang");
    starting.abort();
    hanging.abort();
    engine.shutdown();
}

/// docs/chat.md „Bearbeiten und Löschen“: the calls and what the server refuses.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn editing_and_deleting_on_the_client() {
    let server = serve().await;
    let c = client(&server);
    let status = |e: talk_core::Error| match e {
        talk_core::Error::Http { status, .. } => status,
        talk_core::Error::NotFound => 404,
        other => panic!("{other}"),
    };
    let garden = c.history("garten", None, 50).await.unwrap().messages;
    let find = |text: &str| garden.iter().find(|m| m.message.starts_with(text)).unwrap().id;

    // Edit: the answer is the hidden system message, its parent the new version.
    let sent = c.send_message("garten", "Ich gieße am Samstag", &talk_core::new_reference_id(), None).await.unwrap();
    c.add_reaction("garten", sent.id, "👍").await.unwrap();
    let answer = c.edit_message("garten", sent.id, "Ich gieße am Sonntag, @lena.").await.unwrap();
    assert_eq!(answer.system_message, "message_edited");
    let new = answer.parent.unwrap();
    assert_eq!((new.id, new.rendered_text().as_str()), (sent.id, "Ich gieße am Sonntag, @Lena Hoffmann."));
    assert_eq!(new.source_text(), "Ich gieße am Sonntag, @\"lena\".");
    assert!(new.last_edit_timestamp.is_some_and(|t| t > 0));
    // As on a real server: the answer does not say which reactions are ours ...
    assert!(new.reactions_self.is_empty() && new.reactions["👍"] == 1);
    // ... the live update does.
    let poll = c.poll_messages("garten", sent.id, 0).await.unwrap();
    let hidden = poll.messages.iter().find(|m| m.system_message == "message_edited").unwrap();
    assert_eq!(hidden.parent.as_ref().unwrap().reactions_self, vec!["👍".to_string()]);
    let again = c.history("garten", None, 5).await.unwrap().messages;
    assert!(again.iter().find(|m| m.id == sent.id).unwrap().last_edit_timestamp.is_some());
    let last = c.room("garten").await.unwrap().last_message.unwrap();
    assert_eq!((last.id, last.message.as_str()), (sent.id, "Ich gieße am Sonntag, {mention-user1}."), "not the hidden one");

    // Refused edits.
    let err = c.edit_message("garten", sent.id, "  ").await.unwrap_err();
    assert_eq!(err.ocs_error().as_deref(), Some("message"));
    assert_eq!(message_error_text(MessageOp::Edit, &err), "Die Nachricht darf nicht leer sein");
    let err = c.edit_message("garten", find("Bin dabei!"), "Zu spät").await.unwrap_err();
    assert_eq!(err.ocs_error().as_deref(), Some("age"));
    assert_eq!(message_error_text(MessageOp::Edit, &err), "Nachrichten lassen sich nur 24 Stunden lang bearbeiten");
    let err = c.edit_message("garten", find("Den Gießplan"), "Nicht meine").await.unwrap_err();
    assert_eq!(message_error_text(MessageOp::Edit, &err), "Keine Berechtigung");
    let system = garden.iter().find(|m| m.is_system()).unwrap().id;
    assert_eq!(status(c.edit_message("garten", system, "x").await.unwrap_err()), 405);
    assert_eq!(status(c.edit_message("garten", 999_999, "x").await.unwrap_err()), 404);

    // Delete: own messages; as the owner of a group also others'; never in a one-to-one chat.
    let answer = c.delete_message("garten", sent.id).await.unwrap();
    assert_eq!(answer.system_message, "message_deleted");
    let gone = answer.parent.unwrap();
    assert!(gone.is_deleted() && gone.reactions.is_empty() && gone.last_edit_timestamp.is_none());
    let err = c.delete_message("garten", sent.id).await.unwrap_err();
    assert_eq!(message_error_text(MessageOp::Delete, &err), "Diese Nachricht lässt sich nicht löschen");
    assert_eq!(status(c.delete_message("garten", find("Den Gießplan")).await.unwrap_err()), 403);
    let family = c.history("familie", None, 50).await.unwrap().messages;
    let theirs = family.iter().find(|m| m.actor_id == "paul" && !m.is_system()).unwrap().id;
    assert!(c.delete_message("familie", theirs).await.unwrap().parent.unwrap().is_deleted());
    let one = c.history("lena", None, 50).await.unwrap().messages;
    let theirs = one.iter().find(|m| m.actor_id == "lena").unwrap().id;
    assert_eq!(status(c.delete_message("lena", theirs).await.unwrap_err()), 403);
    // A reply to a deleted message quotes it as deleted.
    let reply = family.iter().find(|m| m.parent.as_ref().is_some_and(|p| p.actor_id == "emil")).unwrap();
    c.delete_message("familie", reply.parent.as_ref().unwrap().id).await.unwrap();
    let family = c.history("familie", None, 50).await.unwrap().messages;
    assert!(family.iter().find(|m| m.id == reply.id).unwrap().parent.as_ref().unwrap().is_deleted());
}

/// The updates of one message the platform was told, in order.
fn updates(sink: &Sink, id: i64) -> Vec<MessageView> {
    let events = sink.0.lock().unwrap();
    let all = events.iter().filter_map(|e| match e {
        Event::MessagesUpdated { messages, .. } => Some(messages.iter().filter(|m| m.id == id).cloned()),
        _ => None,
    });
    all.flatten().collect()
}

/// A message the live update brought (`MessagesAdded`).
async fn arrived(sink: &Sink, text: &'static str) -> MessageView {
    let find = || {
        let events = sink.0.lock().unwrap();
        events.iter().find_map(|e| match e {
            Event::MessagesAdded { messages, .. } => messages.iter().find(|m| m.text == text).cloned(),
            _ => None,
        })
    };
    until(text, || find().is_some()).await;
    find().unwrap()
}

/// docs/chat.md „Bearbeiten und Löschen“: what the platform sees. A changed message
/// keeps its quote and the own reaction, the replies to it get the new quote, and the
/// chat list shows the new text at once.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn editing_and_deleting_in_the_engine() {
    let server = serve().await;
    let dir = std::env::temp_dir().join(format!("talk-demo-test-{}", uuid::Uuid::new_v4().simple()));
    let account = Account { server: server.clone(), user: "demo".into(), app_password: "demo".into() };
    let sink = Arc::new(Sink(Mutex::new(Vec::new())));
    let engine = Engine::new(TalkClient::new(account.clone(), Platform::Cli).unwrap(), sink.clone(), Some(dir.clone()));
    engine.start().await.unwrap();
    let room = |token: &str| engine.rooms().into_iter().find(|r| r.token == token).unwrap();
    let post = |path: &'static str, form: Vec<(&'static str, String)>| {
        let url = format!("{server}{path}");
        async move { assert!(reqwest::Client::new().post(url).form(&form).send().await.unwrap().status().is_success()) }
    };
    let now = chrono::Utc::now().timestamp();

    // Who may do what.
    assert!(room("garten").can_chat && !room("garten").can_delete_others, "a member");
    assert!(room("familie").can_delete_others, "the owner of a group");
    assert!(room("lena").can_chat && !room("lena").can_delete_others, "never in a one-to-one chat");
    let page = engine.open_chat("garten").await.unwrap();
    let old = page.messages.iter().find(|m| m.text.starts_with("Bin dabei!")).unwrap();
    assert!(old.is_own && old.deletable && old.editable && old.edit_until.unwrap() < now, "too old to edit");
    let theirs = page.messages.iter().find(|m| m.text.starts_with("Den Gießplan")).unwrap();
    assert!(theirs.deletable && !theirs.editable);
    let picture = page.messages.iter().find(|m| m.attachment.is_some()).unwrap();
    assert!(picture.deletable && !picture.editable, "not ours");
    let system = page.messages.iter().find(|m| m.kind == "system").unwrap();
    assert!(!system.deletable && !system.editable);
    let (old, theirs) = (old.id, theirs.id);

    // An own message, an own reply to it with an own reaction.
    engine.send("garten", "Erste Fassung", &talk_core::new_reference_id(), None).await.unwrap();
    let first = arrived(&sink, "Erste Fassung").await;
    assert!(first.editable && first.edit_until.unwrap() > now);
    engine.send("garten", "Antwort", &talk_core::new_reference_id(), Some(first.id)).await.unwrap();
    let reply = arrived(&sink, "Antwort").await;
    assert_eq!(reply.reply_to.as_ref().unwrap().text, "Erste Fassung");
    engine.set_reaction("garten", reply.id, Some("👍")).await.unwrap();
    // The live update carries the reply without its quote: it stays.
    until("the reaction over the live update", || updates(&sink, reply.id).len() >= 3).await;
    assert!(updates(&sink, reply.id).iter().all(|m| m.reply_to.is_some()), "a reaction keeps the quote");

    // Edit the quoted message: it changes, and the reply follows with the new quote.
    assert_eq!(engine.edit_source("garten", first.id).await.as_deref(), Some("Erste Fassung"));
    engine.edit_message("garten", first.id, "Zweite Fassung").await.unwrap();
    let new = updates(&sink, first.id).pop().unwrap();
    assert!(new.text == "Zweite Fassung" && new.edited && new.is_own);
    assert_eq!(updates(&sink, reply.id).pop().unwrap().reply_to.unwrap().text, "Zweite Fassung");
    // The same text again asks nobody.
    until("the edit over the live update", || updates(&sink, first.id).len() >= 2).await;
    let hidden = client(&server).history("garten", None, 50).await.unwrap().messages.len();
    engine.edit_message("garten", first.id, " Zweite Fassung ").await.unwrap();
    assert_eq!(client(&server).history("garten", None, 50).await.unwrap().messages.len(), hidden);

    // Edit the reply: quote and own reaction stay, the chat list shows the new text.
    let told = updates(&sink, reply.id).len();
    engine.edit_message("garten", reply.id, "Antwort an @lena").await.unwrap();
    let new = updates(&sink, reply.id).remove(told);
    assert_eq!(new.text, "Antwort an @Lena Hoffmann");
    assert_eq!(new.reply_to.as_ref().unwrap().text, "Zweite Fassung");
    assert_eq!(new.reactions_self, vec!["👍".to_string()]);
    assert_eq!(room("garten").last_message.unwrap().text, "Antwort an @Lena Hoffmann");
    // The live update says the same.
    until("the edit of the reply over the live update", || updates(&sink, reply.id).len() > told + 1).await;
    let new = updates(&sink, reply.id).pop().unwrap();
    assert!(new.text == "Antwort an @Lena Hoffmann" && new.reply_to.is_some() && new.reactions_self.len() == 1);
    assert_eq!(engine.edit_source("garten", reply.id).await.as_deref(), Some("Antwort an @\"lena\""));

    // What the server refuses comes back as a text to show.
    let text = |r: talk_core::Result<()>, op| message_error_text(op, &r.unwrap_err());
    assert_eq!(
        text(engine.edit_message("garten", old, "Zu spät").await, MessageOp::Edit),
        "Nachrichten lassen sich nur 24 Stunden lang bearbeiten"
    );
    assert_eq!(text(engine.delete_message("garten", theirs).await, MessageOp::Delete), "Keine Berechtigung");
    assert_eq!(text(engine.delete_message("garten", -3).await, MessageOp::Delete), "Nachricht nicht gefunden");

    // Delete the quoted message: the reply quotes a deleted message.
    let (told, told_reply) = (updates(&sink, first.id).len(), updates(&sink, reply.id).len());
    engine.delete_message("garten", first.id).await.unwrap();
    let gone = updates(&sink, first.id).remove(told);
    assert!(gone.kind == "deleted" && gone.text.is_empty() && !gone.deletable && !gone.editable);
    let quote = updates(&sink, reply.id).remove(told_reply).reply_to.unwrap();
    assert!(quote.id == first.id && quote.text.is_empty());
    engine.delete_message("garten", first.id).await.unwrap();

    // Somebody else edits and deletes: the same over the live update.
    post("/demo/incoming", vec![("token", "garten".into()), ("text", "Ich bringe Erde mit".into()), ("from", "jonas".into())]).await;
    let theirs = arrived(&sink, "Ich bringe Erde mit").await;
    engine.send("garten", "Danke!", &talk_core::new_reference_id(), Some(theirs.id)).await.unwrap();
    let thanks = arrived(&sink, "Danke!").await;
    engine.set_reaction("garten", theirs.id, Some("❤️")).await.unwrap();
    post("/demo/edit", vec![("token", "garten".into()), ("id", theirs.id.to_string()), ("text", "Ich bringe Kompost mit".into())]).await;
    until("their edit", || updates(&sink, theirs.id).last().is_some_and(|m| m.text == "Ich bringe Kompost mit")).await;
    let new = updates(&sink, theirs.id).pop().unwrap();
    assert!(new.edited && !new.is_own && new.reactions_self == vec!["❤️".to_string()]);
    until("the quote of their edit", || {
        updates(&sink, thanks.id).last().is_some_and(|m| m.reply_to.as_ref().unwrap().text == "Ich bringe Kompost mit")
    })
    .await;
    // The last message of the chat changes: the list follows without a refresh, and
    // nothing is announced.
    post("/demo/incoming", vec![("token", "garten".into()), ("text", "Bis Samstag".into()), ("from", "clara".into())]).await;
    let last = arrived(&sink, "Bis Samstag").await;
    let notices = |sink: &Sink| sink.0.lock().unwrap().iter().filter(|e| matches!(e, Event::MessageNotices { .. })).count();
    let announced = notices(&sink);
    post("/demo/edit", vec![("token", "garten".into()), ("id", last.id.to_string()), ("text", "Bis Sonntag".into())]).await;
    until("the preview of their edit", || room("garten").last_message.is_some_and(|m| m.text == "Bis Sonntag")).await;
    post("/demo/delete", vec![("token", "garten".into()), ("id", last.id.to_string())]).await;
    until("their deletion", || updates(&sink, last.id).last().is_some_and(|m| m.kind == "deleted")).await;
    until("the preview of their deletion", || room("garten").last_message.is_some_and(|m| m.text.is_empty())).await;
    assert_eq!(notices(&sink), announced);
    post("/demo/delete", vec![("token", "garten".into()), ("id", theirs.id.to_string())]).await;
    until("the quote of their deletion", || {
        updates(&sink, thanks.id).last().is_some_and(|m| m.reply_to.as_ref().unwrap().text.is_empty())
    })
    .await;

    // A deleted reply shows no quote.
    engine.delete_message("garten", thanks.id).await.unwrap();
    let gone = updates(&sink, thanks.id).pop().unwrap();
    assert!(gone.kind == "deleted" && gone.reply_to.is_none());

    // All of it is in the cache: a start without the server shows the same.
    engine.shutdown();
    let sink = Arc::new(Sink(Mutex::new(Vec::new())));
    let engine = Engine::new(TalkClient::new(account, Platform::Cli).unwrap(), sink, Some(dir.clone()));
    let cached = engine.cached_chat("garten").messages;
    let by_id = |id: i64| cached.iter().find(|m| m.id == id).unwrap();
    assert_eq!(by_id(first.id).kind, "deleted");
    let kept = by_id(reply.id);
    assert_eq!(kept.text, "Antwort an @Lena Hoffmann");
    assert!(kept.edited && kept.reactions_self == vec!["👍".to_string()]);
    assert!(kept.reply_to.as_ref().unwrap().id == first.id && kept.reply_to.as_ref().unwrap().text.is_empty());
    assert!(by_id(thanks.id).kind == "deleted" && by_id(thanks.id).reply_to.is_none());
    assert_eq!(by_id(last.id).kind, "deleted");
    assert!(cached.iter().all(|m| !m.system_message.starts_with("message_")), "the hidden messages stay hidden");
    engine.shutdown();
    let _ = std::fs::remove_dir_all(dir);
}

/// Offline nothing is kept for later: the platform gets the text to show.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn editing_without_the_server() {
    let mut server = Outage::server().await;
    let (engine, _sink) = server.engine();
    engine.start().await.unwrap();
    let page = engine.open_chat("lena").await.unwrap();
    let own = page.messages.iter().rev().find(|m| m.is_own).unwrap().id;
    // The server only stops once no request is under way: not with the chat's long poll.
    engine.close_chat("lena");
    server.stop().await;
    let err = engine.edit_message("lena", own, "Neu").await.unwrap_err();
    assert_eq!(message_error_text(MessageOp::Edit, &err), "Keine Verbindung – Nachricht nicht geändert");
    let err = engine.delete_message("lena", own).await.unwrap_err();
    assert_eq!(message_error_text(MessageOp::Delete, &err), "Keine Verbindung – Nachricht nicht gelöscht");
    engine.shutdown();
}

/// Somebody else writes into a conversation (test hook of the demo server).
async fn incoming(server: &str, token: &str, text: &str) {
    let r = reqwest::Client::new().post(format!("{server}/demo/incoming")).form(&[("token", token), ("text", text)]).send().await.unwrap();
    assert_eq!(r.status(), 201);
}

/// How often the platform was handed a message with this text as new.
fn added(sink: &Sink, text: &str) -> usize {
    let events = sink.0.lock().unwrap();
    let all = events.iter().filter_map(|e| match e {
        Event::MessagesAdded { messages, .. } => Some(messages.iter().filter(|m| m.text == text).count()),
        _ => None,
    });
    all.sum()
}

fn chat_push(token: &str) -> PushMessage {
    PushMessage::Chat { token: token.into(), notification_id: 1, subject: String::new() }
}

/// docs/notifications.md „Nach dem Antippen“: the push for a message fetches the chat, so
/// its first frame shows the message when the user follows the notification.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_push_fetches_the_chat_ahead() {
    let server = serve().await;
    let dir = std::env::temp_dir().join(format!("talk-demo-test-{}", uuid::Uuid::new_v4().simple()));
    let account = Account { server: server.clone(), user: "demo".into(), app_password: "demo".into() };
    let sink = Arc::new(Sink(Mutex::new(Vec::new())));
    let engine = Engine::new(TalkClient::new(account, Platform::Cli).unwrap(), sink.clone(), Some(dir.clone()));
    engine.start().await.unwrap();
    let texts = |token: &str, text: &str| engine.cached_chat(token).messages.iter().filter(|m| m.text == text).count();

    // A chat that was open before: its cache ends with the last visit.
    engine.open_chat("garten").await.unwrap();
    engine.close_chat("garten");
    incoming(&server, "garten", "Wer gießt morgen?").await;
    assert_eq!(texts("garten", "Wer gießt morgen?"), 0, "nothing fetches a closed chat by itself");
    // The app slept meanwhile: the push is the first thing that happens.
    engine.pretend_paused();
    engine.handle_push(chat_push("garten"), None).await;
    assert_eq!(texts("garten", "Wer gießt morgen?"), 1, "the cache has the message when the call returns");
    // Opening the chat shows it once and announces nothing as new.
    let page = engine.open_chat("garten").await.unwrap();
    assert_eq!(page.messages.iter().filter(|m| m.text == "Wer gießt morgen?").count(), 1);
    assert!(!page.reset);
    assert_eq!(added(&sink, "Wer gießt morgen?"), 0);

    // The chat is open and the app slept: its poll starts over and brings the message, and
    // the call returns when it is there. The push adds nothing to it.
    engine.pretend_paused();
    incoming(&server, "garten", "Ich kann am Abend.").await;
    engine.handle_push(chat_push("garten"), None).await;
    assert_eq!(added(&sink, "Ich kann am Abend."), 1, "the open chat has the message when the call returns");
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(added(&sink, "Ich kann am Abend."), 1);
    // The poll was faster than the push: nothing is left to wait for.
    let started = std::time::Instant::now();
    engine.handle_push(chat_push("garten"), None).await;
    assert!(started.elapsed() < Duration::from_secs(1), "waited {:?} for nothing", started.elapsed());
    engine.close_chat("garten");

    // A change comes the same way: somebody edits their message while the chat is closed.
    let theirs = engine.cached_chat("garten").messages.iter().find(|m| m.text == "Wer gießt morgen?").unwrap().id;
    let r = reqwest::Client::new()
        .post(format!("{server}/demo/edit"))
        .form(&[("token", "garten"), ("id", &theirs.to_string()), ("text", "Wer gießt übermorgen?")])
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success());
    engine.handle_push(chat_push("garten"), None).await;
    assert_eq!(engine.cached_chat("garten").messages.iter().find(|m| m.id == theirs).unwrap().text, "Wer gießt übermorgen?");

    // A chat that was never opened has no cache: the push fetches its newest page.
    assert!(engine.cached_chat("familie").messages.is_empty());
    incoming(&server, "familie", "Sonntag bei uns?").await;
    engine.handle_push(chat_push("familie"), None).await;
    let cached = engine.cached_chat("familie");
    assert_eq!(cached.messages.last().unwrap().text, "Sonntag bei uns?");
    assert!(cached.messages.len() > 1, "the page, not the one message: nothing is missing before it");

    // A push for a chat the server does not know changes nothing and does not fail.
    engine.handle_push(chat_push("gibtesnicht"), None).await;
    assert!(engine.cached_chat("gibtesnicht").messages.is_empty());
    engine.shutdown();
    let _ = std::fs::remove_dir_all(dir);
}

/// Without push (desktop) the room list announces the message: what is announced is
/// fetched, so a click on the notification finds it in the chat.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_notice_fetches_the_chat_ahead() {
    let server = serve().await;
    let dir = std::env::temp_dir().join(format!("talk-demo-test-{}", uuid::Uuid::new_v4().simple()));
    let account = Account { server: server.clone(), user: "demo".into(), app_password: "demo".into() };
    let sink = Arc::new(Sink(Mutex::new(Vec::new())));
    let engine = Engine::new(TalkClient::new(account, Platform::Cli).unwrap(), sink.clone(), Some(dir.clone()));
    engine.start().await.unwrap();
    engine.check_incoming_calls().await;

    incoming(&server, "hilde", "Bist du morgen da?").await;
    engine.refresh_rooms(false).await.unwrap();
    assert_eq!(announced(&sink).0, ["hilde"]);
    until("the announced chat is in the cache", || {
        engine.cached_chat("hilde").messages.iter().any(|m| m.text == "Bist du morgen da?")
    })
    .await;
    // The chat was closed all along: nothing was handed on as new.
    assert_eq!(added(&sink, "Bist du morgen da?"), 0);
    engine.shutdown();
    let _ = std::fs::remove_dir_all(dir);
}

/// An open chat after a pause: its poll starts over on a new connection and neither
/// loses nor repeats a message. And a poll that pauses after failures tries again as
/// soon as the platform says the network is back.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_open_chat_catches_up_when_the_app_returns() {
    let mut outage = Outage::server().await;
    let server = outage.url();
    let dir = std::env::temp_dir().join(format!("talk-demo-test-{}", uuid::Uuid::new_v4().simple()));
    let account = Account { server: server.clone(), user: "demo".into(), app_password: "demo".into() };
    let sink = Arc::new(Sink(Mutex::new(Vec::new())));
    let engine = Engine::new(TalkClient::new(account, Platform::Cli).unwrap(), sink.clone(), Some(dir.clone()));
    engine.start().await.unwrap();
    engine.open_chat("garten").await.unwrap();

    // The app comes back after a pause while a message is under way.
    engine.pretend_paused();
    incoming(&server, "garten", "Eins").await;
    engine.network_changed();
    incoming(&server, "garten", "Zwei").await;
    arrived(&sink, "Eins").await;
    arrived(&sink, "Zwei").await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!((added(&sink, "Eins"), added(&sink, "Zwei")), (1, 1));

    // The server goes away with the chat closed, and the chat is opened without it: the
    // poll fails and pauses longer each time (1 s, 2 s, 4 s, then 8 s).
    engine.close_chat("garten");
    outage.stop().await;
    assert!(engine.open_chat("garten").await.is_err());
    tokio::time::sleep(Duration::from_secs(8)).await;
    outage.start().await;
    incoming(&server, "garten", "Drei").await;
    engine.network_changed();
    // Without the wake-up the poll would sleep on for several seconds.
    let woke = tokio::time::timeout(Duration::from_millis(2500), arrived(&sink, "Drei")).await;
    assert!(woke.is_ok(), "the poll waited out its pause");
    assert_eq!(added(&sink, "Drei"), 1);
    engine.shutdown();
    let _ = std::fs::remove_dir_all(dir);
}
