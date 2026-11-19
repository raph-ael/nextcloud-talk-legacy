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

    let pending = engine.enqueue_text("lena", "Bis gleich!", Vec::new(), None, "ref-outage");
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
    assert_eq!(engine.edit_source("garten", first.id).await.unwrap().text, "Erste Fassung");
    engine.edit_message("garten", first.id, "Zweite Fassung", &[]).await.unwrap();
    let new = updates(&sink, first.id).pop().unwrap();
    assert!(new.text == "Zweite Fassung" && new.edited && new.is_own);
    assert_eq!(updates(&sink, reply.id).pop().unwrap().reply_to.unwrap().text, "Zweite Fassung");
    // The same text again asks nobody.
    until("the edit over the live update", || updates(&sink, first.id).len() >= 2).await;
    let hidden = client(&server).history("garten", None, 50).await.unwrap().messages.len();
    engine.edit_message("garten", first.id, " Zweite Fassung ", &[]).await.unwrap();
    assert_eq!(client(&server).history("garten", None, 50).await.unwrap().messages.len(), hidden);

    // Edit the reply: quote and own reaction stay, the chat list shows the new text.
    let told = updates(&sink, reply.id).len();
    engine.edit_message("garten", reply.id, "Antwort an @lena", &[]).await.unwrap();
    let new = updates(&sink, reply.id).remove(told);
    assert_eq!(new.text, "Antwort an @Lena Hoffmann");
    assert_eq!(new.reply_to.as_ref().unwrap().text, "Zweite Fassung");
    assert_eq!(new.reactions_self, vec!["👍".to_string()]);
    assert_eq!(room("garten").last_message.unwrap().text, "Antwort an @Lena Hoffmann");
    // The live update says the same.
    until("the edit of the reply over the live update", || updates(&sink, reply.id).len() > told + 1).await;
    let new = updates(&sink, reply.id).pop().unwrap();
    assert!(new.text == "Antwort an @Lena Hoffmann" && new.reply_to.is_some() && new.reactions_self.len() == 1);
    // The input field gets the name, and beside it who that is.
    let draft = engine.edit_source("garten", reply.id).await.unwrap();
    assert_eq!(draft.text, "Antwort an @Lena Hoffmann");
    assert_eq!(draft.mentions, [talk_core::MentionPick { label: "Lena Hoffmann".into(), mention_id: "lena".into() }]);

    // What the server refuses comes back as a text to show.
    let text = |r: talk_core::Result<()>, op| message_error_text(op, &r.unwrap_err());
    assert_eq!(
        text(engine.edit_message("garten", old, "Zu spät", &[]).await, MessageOp::Edit),
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
    let err = engine.edit_message("lena", own, "Neu", &[]).await.unwrap_err();
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

/// Somebody else reacts to a message, or takes the reaction back (test hook of the demo server).
async fn reacts(server: &str, token: &str, id: i64, from: &str, emoji: &str) {
    let id = id.to_string();
    let form = [("token", token), ("id", &id), ("from", from), ("emoji", emoji)];
    let r = reqwest::Client::new().post(format!("{server}/demo/react")).form(&form).send().await.unwrap();
    assert!(r.status().is_success(), "{}", r.status());
}

/// docs/chat.md „Reaktionen“, fremde Reaktionen im offenen Chat: Talk answers a waiting long poll from
/// its memory of "nothing new", and a new reaction does not clear that memory. The open
/// chat looks past it every few seconds, and at once when a push announces the reaction.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_reaction_shows_in_the_open_chat() {
    let server = serve().await;
    let dir = std::env::temp_dir().join(format!("talk-demo-test-{}", uuid::Uuid::new_v4().simple()));
    let account = Account { server: server.clone(), user: "demo".into(), app_password: "demo".into() };
    let sink = Arc::new(Sink(Mutex::new(Vec::new())));
    let engine = Engine::new(TalkClient::new(account, Platform::Cli).unwrap(), sink.clone(), Some(dir.clone()));
    engine.start().await.unwrap();
    let page = engine.open_chat("garten").await.unwrap();
    let id = page.messages.last().unwrap().id;
    let count = |emoji: &str| updates(&sink, id).last().and_then(|m| m.reactions.get(emoji).copied()).unwrap_or(0);
    // The poll waits now, the server remembers that nothing is newer.
    tokio::time::sleep(Duration::from_secs(1)).await;

    // Nothing announces the reaction: the look past the memory brings it.
    let started = std::time::Instant::now();
    reacts(&server, "garten", id, "jonas", "👍").await;
    until("the reaction shows", || count("👍") == 1).await;
    assert!(started.elapsed() < Duration::from_secs(7), "took {:?}", started.elapsed());

    // A push announces the next one: it is there when the call returns.
    let started = std::time::Instant::now();
    reacts(&server, "garten", id, "lena", "👍").await;
    engine.handle_push(chat_push("garten"), None).await;
    assert_eq!(count("👍"), 2, "the open chat has the reaction when the call returns");
    assert!(started.elapsed() < Duration::from_secs(2), "took {:?}", started.elapsed());

    // Taking one back wakes the poll at the server, as before.
    let started = std::time::Instant::now();
    reacts_back(&server, "garten", id, "jonas", "👍").await;
    until("the reaction goes", || count("👍") == 1).await;
    assert!(started.elapsed() < Duration::from_secs(2), "took {:?}", started.elapsed());

    // Messages come once, whichever of the two requests brings them.
    incoming(&server, "garten", "Schon gesehen.").await;
    arrived(&sink, "Schon gesehen.").await;
    tokio::time::sleep(Duration::from_secs(6)).await;
    assert_eq!(added(&sink, "Schon gesehen."), 1);
    engine.close_chat("garten");

    // The chat is closed and somebody else's app waits with the id this one knows: the
    // push still fetches the reaction.
    let known = engine.cached_chat("garten").messages.last().unwrap().id;
    let other = client(&server);
    let waiting = tokio::spawn(async move { other.poll_messages("garten", known, 20).await });
    tokio::time::sleep(Duration::from_millis(500)).await;
    reacts(&server, "garten", id, "clara", "🌱").await;
    engine.handle_push(chat_push("garten"), None).await;
    let cached = engine.cached_chat("garten");
    assert_eq!(cached.messages.iter().find(|m| m.id == id).unwrap().reactions.get("🌱"), Some(&1));
    waiting.abort();
    engine.shutdown();
    let _ = std::fs::remove_dir_all(dir);
}

async fn reacts_back(server: &str, token: &str, id: i64, from: &str, emoji: &str) {
    let id = id.to_string();
    let form = [("token", token), ("id", &id), ("from", from), ("emoji", emoji), ("remove", "1")];
    let r = reqwest::Client::new().post(format!("{server}/demo/react")).form(&form).send().await.unwrap();
    assert!(r.status().is_success(), "{}", r.status());
}

/// docs/attachments.md „Standort“: a place is sent through the outbox like a text and comes
/// back with its coordinates; somebody else's place carries them too, in the chat and in
/// the preview of the list.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_place_is_sent_and_read() {
    let server = serve().await;
    let dir = std::env::temp_dir().join(format!("talk-demo-test-{}", uuid::Uuid::new_v4().simple()));
    let account = Account { server: server.clone(), user: "demo".into(), app_password: "demo".into() };
    let sink = Arc::new(Sink(Mutex::new(Vec::new())));
    let engine = Engine::new(TalkClient::new(account, Platform::Cli).unwrap(), sink.clone(), Some(dir.clone()));
    engine.start().await.unwrap();

    // A place in the history: coordinates, the name as text, nothing to edit.
    let page = engine.open_chat("sommerfest").await.unwrap();
    let park = page.messages.iter().find(|m| m.location.is_some()).expect("the place of the fixtures");
    let l = park.location.clone().unwrap();
    assert_eq!((l.latitude, l.longitude), (52.5283, 13.4356));
    assert_eq!(park.text, "Volkspark Friedrichshain, Berlin");
    assert!(park.attachment.is_none() && !park.editable && park.kind == "text");

    // Sending: pending at once, then the server's message replaces it.
    let pending = engine.enqueue_location("sommerfest", 50.927054, 11.589237, "  Markt 1, Jena ", "ref-place");
    assert!(pending.message.id < 0 && pending.message.text == "Markt 1, Jena");
    assert_eq!(pending.message.location.as_ref().unwrap().name, "Markt 1, Jena");
    let sent = arrived(&sink, "Markt 1, Jena").await;
    assert!(sent.id > 0 && sent.is_own && sent.reference_id == "ref-place");
    let l = sent.location.unwrap();
    assert_eq!((l.latitude, l.longitude, l.name.as_str()), (50.927054, 11.589237, "Markt 1, Jena"));
    until("the outbox is empty", || engine.pending("sommerfest").is_empty()).await;

    // Without a name the server still gets one (it refuses an object without).
    engine.enqueue_location("sommerfest", -33.9, 151.2, "", "ref-nameless");
    let sent = arrived(&sink, "-33.900000,151.200000").await;
    assert_eq!(sent.location.map(|l| (l.latitude, l.longitude)), Some((-33.9, 151.2)));

    // Somebody else's place: in the chat and in the list.
    let r = reqwest::Client::new()
        .post(format!("{server}/demo/incoming"))
        .form(&[("token", "sommerfest"), ("text", "Am Brunnen"), ("lat", "52.52"), ("lon", "13.405"), ("from", "paul")])
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 201);
    let theirs = arrived(&sink, "Am Brunnen").await;
    assert!(!theirs.is_own && theirs.location.is_some_and(|l| l.latitude == 52.52 && l.longitude == 13.405));
    let preview = || engine.rooms().into_iter().find(|r| r.token == "sommerfest").and_then(|r| r.last_message);
    until("the preview of the list", || preview().is_some_and(|m| m.text == "Am Brunnen" && m.location.is_some())).await;

    engine.close_chat("sommerfest");
    engine.shutdown();
    let _ = std::fs::remove_dir_all(dir);
}

/// docs/attachments.md „Kontakt“: a contact goes out as a vCard file named after the
/// person and comes back with the name the server read from it; somebody else's contact
/// carries it too, in the chat and in the preview of the list.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_contact_is_sent_and_read() {
    use talk_core::{ContactCard, ContactValue};
    let server = serve().await;
    let dir = std::env::temp_dir().join(format!("talk-demo-test-{}", uuid::Uuid::new_v4().simple()));
    let account = Account { server: server.clone(), user: "demo".into(), app_password: "demo".into() };
    let sink = Arc::new(Sink(Mutex::new(Vec::new())));
    let engine = Engine::new(TalkClient::new(account, Platform::Cli).unwrap(), sink.clone(), Some(dir.clone()));
    engine.start().await.unwrap();
    let contact = |name: &'static str| {
        let events = sink.0.lock().unwrap();
        events.iter().find_map(|e| match e {
            Event::MessagesAdded { messages, .. } => {
                messages.iter().find(|m| m.attachment.as_ref().is_some_and(|a| a.contact_name == name)).cloned()
            }
            _ => None,
        })
    };

    // A contact in the history: a file with the person's name, other files have none.
    let page = engine.open_chat("sommerfest").await.unwrap();
    let karla = page.messages.iter().find_map(|m| m.attachment.clone().filter(|a| !a.contact_name.is_empty()));
    let karla = karla.expect("the contact of the fixtures");
    assert_eq!((karla.contact_name.as_str(), karla.mimetype.as_str()), ("Karla Brandt", "text/vcard"));
    let garden = engine.open_chat("garten").await.unwrap().messages.into_iter().find_map(|m| m.attachment).unwrap();
    assert!(garden.is_image() && garden.contact_name.is_empty());
    engine.close_chat("garten");

    // Sending: pending at once under the person's name, then the server's message.
    let card = ContactCard {
        name: "Hanna Lindqvist".into(),
        given_name: "Hanna".into(),
        family_name: "Lindqvist".into(),
        phones: vec![ContactValue { kind: "mobile".into(), value: "+49 30 23125 117".into() }],
        ..Default::default()
    };
    let pending = engine.enqueue_contact("sommerfest", &card, None, "ref-contact").unwrap();
    let a = pending.message.attachment.clone().unwrap();
    assert!(pending.message.id < 0 && pending.message.text.is_empty());
    assert_eq!((a.name.as_str(), a.contact_name.as_str(), a.mimetype.as_str()), ("Hanna Lindqvist.vcf", "Hanna Lindqvist", "text/vcard"));
    until("the contact arrives", || contact("Hanna Lindqvist").is_some()).await;
    let sent = contact("Hanna Lindqvist").unwrap();
    assert!(sent.id > 0 && sent.is_own && sent.reference_id == "ref-contact" && sent.text.is_empty());
    until("the outbox is empty", || engine.pending("sommerfest").is_empty()).await;
    // The file on the server is the card.
    let path = sent.attachment.unwrap().path;
    let stored = reqwest::Client::new()
        .get(format!("{server}/remote.php/dav/files/demo/{path}"))
        .basic_auth("demo", Some("demo"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert_eq!(stored, card.vcard());

    // A name no file system takes: the file gets a clean one, the card keeps the name.
    let odd = ContactCard { name: "A/B: \"C\"?".into(), ..Default::default() };
    let pending = engine.enqueue_contact("sommerfest", &odd, None, "ref-odd").unwrap();
    assert_eq!(pending.message.attachment.unwrap().name, "A B C.vcf");
    until("the odd one arrives", || contact("A/B: \"C\"?").is_some()).await;

    // Somebody else's contact: in the chat and in the list.
    let r = reqwest::Client::new()
        .post(format!("{server}/demo/incoming"))
        .form(&[("token", "sommerfest"), ("text", "Ole Petersen"), ("contact", "1"), ("from", "paul")])
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 201);
    until("their contact arrives", || contact("Ole Petersen").is_some()).await;
    assert!(!contact("Ole Petersen").unwrap().is_own);
    let preview = || engine.rooms().into_iter().find(|r| r.token == "sommerfest").and_then(|r| r.last_message);
    until("the preview of the list", || {
        preview().and_then(|m| m.attachment).is_some_and(|a| a.contact_name == "Ole Petersen")
    })
    .await;

    engine.close_chat("sommerfest");
    engine.shutdown();
    let _ = std::fs::remove_dir_all(dir);
}

/// docs/attachments.md „Datei senden“: a file whose first attempt ran without network is
/// uploaded afterwards, also when the folder already holds a file of the same name and
/// size. That one is somebody else's and is never shared in its place.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_file_of_the_same_name_and_size_is_not_taken_for_ours() {
    let mut server = Outage::server().await;
    let dir = std::env::temp_dir().join(format!("talk-demo-test-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&dir).unwrap();
    let account = Account { server: server.url(), user: "demo".into(), app_password: "demo".into() };
    let sink = Arc::new(Sink(Mutex::new(Vec::new())));
    let engine = Engine::new(TalkClient::new(account, Platform::Cli).unwrap(), sink.clone(), Some(dir.clone()));
    engine.start().await.unwrap();
    let send = |content: &str, reference: &str| {
        let path = dir.join(reference);
        std::fs::write(&path, content).unwrap();
        engine
            .enqueue_file("sommerfest", &path.to_string_lossy(), "notiz.txt", "text/plain", 0, 0, "", Vec::new(), None, reference)
            .unwrap();
    };
    // Where the message with this reference points to, and what lies there.
    let base = server.url();
    let sent = |reference: &'static str| {
        let (engine, base) = (engine.clone(), base.clone());
        async move {
            let batch = engine.client().history("sommerfest", None, 50).await.unwrap();
            let m = batch.messages.iter().find(|m| m.reference_id == reference).expect("sent");
            let path = m.attachment().unwrap().path;
            let url = format!("{base}/remote.php/dav/files/demo/{path}");
            let r = reqwest::Client::new().get(url).basic_auth("demo", Some("demo")).send().await.unwrap();
            assert!(r.status().is_success(), "{path}: {}", r.status());
            (path, r.text().await.unwrap())
        }
    };

    // The older file, sent with network.
    send("alt-alt", "ref-old");
    until("the first file is sent", || engine.pending("sommerfest").is_empty()).await;
    let old = sent("ref-old").await;
    assert_eq!(old.1, "alt-alt");

    // Same name, same size, first attempt without the server.
    server.stop().await;
    send("neu-neu", "ref-new");
    until("waiting for the network", || {
        engine.pending("sommerfest").iter().any(|p| p.state == talk_core::PendingState::Waiting)
    })
    .await;
    server.start().await;
    until("the second file is sent", || engine.pending("sommerfest").is_empty()).await;
    let new = sent("ref-new").await;
    assert_ne!(new.0, old.0);
    assert_eq!(new.1, "neu-neu");
    assert_eq!(sent("ref-old").await.1, "alt-alt");

    engine.shutdown();
    let _ = std::fs::remove_dir_all(dir);
}

fn snippet_text(hit: &talk_core::SearchHit) -> String {
    hit.snippet.iter().map(|p| if p.hit { format!("[{}]", p.text) } else { p.text.clone() }).collect()
}

/// docs/search.md: the history of all chats is fetched in the background and searched on
/// the device; a hit opens the chat at its message.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_whole_history_is_searchable() {
    let server = serve().await;
    let dir = std::env::temp_dir().join(format!("talk-demo-test-{}", uuid::Uuid::new_v4().simple()));
    let account = Account { server: server.clone(), user: "demo".into(), app_password: "demo".into() };
    let sink = Arc::new(Sink(Mutex::new(Vec::new())));
    let engine = Engine::new(TalkClient::new(account.clone(), Platform::Cli).unwrap(), sink.clone(), Some(dir.clone()));
    engine.start().await.unwrap();
    let post = |path: &'static str, form: Vec<(&'static str, String)>| {
        let url = format!("{server}{path}");
        async move { assert!(reqwest::Client::new().post(url).form(&form).send().await.unwrap().status().is_success()) }
    };
    let total = |query: &str, token: Option<&str>| engine.search_messages(query, token, 50).total;
    let loaded = || {
        let s = engine.search_status();
        s.chats_total > 0 && s.chats_done == s.chats_total
    };

    // Before the history is there, the server helps out: the old message is found, with
    // its sender from the title and the word marked.
    let early = engine.search_messages("Quittenmarmelade", None, 50);
    assert!(!early.complete && early.hits.is_empty() && early.chats_done < early.chats_total);
    let early = engine.search_messages_everywhere("Quittenmarmelade", None, 50).await.unwrap();
    assert_eq!(early.total, 1);
    assert_eq!(early.hits[0].token, "hofladen");
    assert!(snippet_text(&early.hits[0]).contains("Die [Quittenmarmelade] ist fertig"));
    assert!(!early.hits[0].actor_name.is_empty() && !early.hits[0].actor_name.contains("Hofladen"));
    // The last message of every chat is searchable from the first look at the list.
    assert_eq!(total("neuen Ort", Some("hofladen")), 1);

    for _ in 0..300 {
        if loaded() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let status = engine.search_status();
    assert!(loaded(), "the history of all chats: {status:?}");
    assert!(status.messages > 1200 && status.chats_capped == 0);

    // One hit far back in the archived chat, found without the server.
    let found = engine.search_messages("quittenmarmelade", None, 50);
    assert!(found.complete && found.total == 1 && found.chats_done == found.chats_total);
    let hit = found.hits[0].clone();
    assert_eq!((hit.token.as_str(), hit.room_name.as_str()), ("hofladen", "Hofladen-Archiv"));
    assert!(hit.show_actor && !hit.is_own && !hit.actor_name.is_empty());
    assert!(snippet_text(&hit).starts_with("Die [Quittenmarmelade] ist fertig"));
    assert_eq!(engine.search_messages_everywhere("quittenmarmelade", None, 50).await.unwrap().total, 1);
    // Parts of words in any order, without accents and capitals; in one chat or in all.
    assert_eq!(total("buro HUPFBURG", None), 1);
    assert_eq!(total("hüpfburg", Some("hofladen")), 1);
    assert_eq!(total("hüpfburg", Some("garten")), 0);
    // A chat counts all its matches; all chats give the newest and say that there are more.
    assert!(total("notiz", Some("hofladen")) >= 1200);
    let many = engine.search_messages("notiz", None, 50);
    assert!(many.hits.len() == 50 && many.total == 50 && many.more);
    assert!(many.hits.windows(2).all(|w| w[0].timestamp >= w[1].timestamp), "newest first");
    assert!(!engine.search_messages("quittenmarmelade", None, 50).more);
    assert!(total("ie", Some("sommerfest")) >= 2, "two characters");
    assert_eq!(total("x", None), 0);
    // A contact by its name, a place by its name, a picture by its file name.
    assert_eq!(total("karla brandt", Some("sommerfest")), 1);
    assert_eq!(total("volkspark", Some("sommerfest")), 1);
    let garden = engine.open_chat("garten").await.unwrap();
    let picture = garden.messages.iter().find_map(|m| m.attachment.clone()).unwrap();
    assert!(total(&picture.name, Some("garten")) >= 1);
    engine.close_chat("garten");
    // A chat's name finds the archived chat too, in its own section.
    let sections = engine.room_sections("hofladen", None);
    assert_eq!(sections.iter().map(|s| s.kind.as_str()).collect::<Vec<_>>(), ["archived"]);
    assert!(engine.room_sections("", None).iter().all(|s| s.rooms.iter().all(|r| r.token != "hofladen")));

    // Opening the chat at the hit: a window from the server (the chat was never open),
    // 36 messages before it, the message, 50 after it.
    let page = engine.messages_around("hofladen", hit.message_id).await.unwrap();
    assert_eq!(page.messages.iter().position(|m| m.id == hit.message_id), Some(36));
    assert_eq!(page.messages.len(), 87);
    assert!(page.has_newer && !page.has_more);
    // Paging on to the end of the chat.
    let (mut newest, mut seen, mut last) = (page.newest_id.unwrap(), page.messages.len(), String::new());
    for _ in 0..40 {
        let next = engine.load_newer("hofladen", newest).await.unwrap();
        assert!(next.messages.iter().all(|m| m.id > newest));
        seen += next.messages.len();
        if let Some(m) = next.messages.last() {
            last = m.text.clone();
        }
        newest = next.newest_id.unwrap();
        if !next.has_newer {
            break;
        }
    }
    assert_eq!(seen, 1202);
    assert!(last.starts_with("Danke für die Jahre"));
    // In the middle both directions go on.
    let middle = engine.search_messages("hüpfburg", Some("hofladen"), 1).hits[0].message_id;
    let page = engine.messages_around("hofladen", middle).await.unwrap();
    assert!(page.has_more && page.has_newer && page.messages.len() == 101);
    let older = engine.load_older("hofladen", page.oldest_id.unwrap()).await.unwrap();
    assert!(older.messages.len() == 50 && older.messages.iter().all(|m| m.id < page.oldest_id.unwrap()));
    // An open chat has its end in the cache: the window comes from there.
    let end = engine.open_chat("hofladen").await.unwrap();
    let near = end.messages[end.messages.len() - 10].id;
    let page = engine.messages_around("hofladen", near).await.unwrap();
    assert!(!page.has_newer && page.has_more);
    assert_eq!(page.messages.last().unwrap().id, end.messages.last().unwrap().id);
    assert_eq!(page.messages.iter().filter(|m| m.id > near).count(), 9);
    let after = engine.load_newer("hofladen", near).await.unwrap();
    assert!(!after.has_newer && after.messages.len() == 9);
    assert!(matches!(engine.messages_around("hofladen", 9_999_999).await, Err(talk_core::Error::NotFound)));
    engine.close_chat("hofladen");

    // A new message in an open chat is found at once; an edit and a deletion reach the
    // index with the live update.
    engine.open_chat("sommerfest").await.unwrap();
    assert_eq!(total("bierbänke", None), 1);
    post("/demo/incoming", vec![("token", "sommerfest".into()), ("text", "Wer bringt die Lampions mit?".into())]).await;
    until("the new message is searchable", || total("lampions", Some("sommerfest")) == 1).await;
    let lanterns = engine.search_messages("lampions", None, 1).hits[0].message_id;
    post("/demo/edit", vec![("token", "sommerfest".into()), ("id", lanterns.to_string()), ("text", "Wer bringt die Girlanden mit?".into())]).await;
    until("the edit is searchable", || total("girlanden", None) == 1 && total("lampions", None) == 0).await;
    post("/demo/delete", vec![("token", "sommerfest".into()), ("id", lanterns.to_string())]).await;
    until("the deleted message is gone", || total("girlanden", None) == 0).await;
    engine.close_chat("sommerfest");

    // A chat that is not open: its new message comes with the chat list, an edit with
    // the message after it.
    post("/demo/incoming", vec![("token", "hofladen".into()), ("text", "Erste Fassung zum Umzug".into())]).await;
    engine.refresh_rooms(false).await.unwrap();
    until("the message of the closed chat", || total("erste fassung umzug", None) == 1).await;
    let moving = engine.search_messages("erste fassung umzug", None, 1).hits[0].message_id;
    post("/demo/edit", vec![("token", "hofladen".into()), ("id", moving.to_string()), ("text", "Zweite Fassung zum Umzug".into())]).await;
    post("/demo/incoming", vec![("token", "hofladen".into()), ("text", "Und noch ein Nachtrag".into())]).await;
    engine.refresh_rooms(false).await.unwrap();
    until("the edit in the closed chat", || total("zweite fassung", None) == 1 && total("erste fassung", None) == 0).await;
    assert_eq!(total("nachtrag", Some("hofladen")), 1);

    // After a restart everything is there at once, without a request.
    engine.shutdown();
    drop(engine);
    let again = Engine::new(TalkClient::new(account, Platform::Cli).unwrap(), sink.clone(), Some(dir.clone()));
    let found = again.search_messages("quittenmarmelade", None, 50);
    assert!(found.total == 1 && found.complete);
    // Signing out forgets it.
    again.logout().await.unwrap();
    until("the index is empty", || again.search_status().messages == 0).await;
    let _ = std::fs::remove_dir_all(&dir);
}

/// docs/mentions.md: the suggestions of a chat, a mention on its way to the server and
/// back into the bubble and the input field, and the list behind the "@" button.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mentions_from_the_suggestions_to_the_bubble() {
    let server = serve().await;
    let dir = std::env::temp_dir().join(format!("talk-demo-test-{}", uuid::Uuid::new_v4().simple()));
    let account = Account { server: server.clone(), user: "demo".into(), app_password: "demo".into() };
    let sink = Arc::new(Sink(Mutex::new(Vec::new())));
    let engine = Engine::new(TalkClient::new(account.clone(), Platform::Cli).unwrap(), sink.clone(), Some(dir.clone()));
    engine.start().await.unwrap();
    let post = |form: Vec<(&'static str, &'static str)>| {
        let url = format!("{server}/demo/incoming");
        async move { assert!(reqwest::Client::new().post(url).form(&form).send().await.unwrap().status().is_success()) }
    };
    let labels = |list: &[talk_core::MentionCandidate]| list.iter().map(|c| c.label.clone()).collect::<Vec<_>>();
    let kinds = |m: &MessageView| m.mentions.iter().map(|x| (x.label.clone(), x.kind, x.is_self)).collect::<Vec<_>>();

    // Who can be mentioned: who wrote last first, not the user, then group and team,
    // everybody last and under its own name.
    let all = engine.mention_candidates("sommerfest", "", 50).await;
    assert_eq!(
        labels(&all),
        ["Lena Hoffmann", "Jonas Richter", "Sophie Neumann", "Paul Lange", "Tobias Krüger", "Helferkreis", "Orga-Team", "Alle"]
    );
    assert_eq!(labels(&engine.mention_candidates("sommerfest", "KRÜ", 50).await), ["Tobias Krüger"]);
    assert_eq!(labels(&engine.mention_candidates("sommerfest", "kru", 50).await), ["Tobias Krüger"]);
    assert_eq!(labels(&engine.mention_candidates("sommerfest", "al", 50).await), ["Alle"]);
    assert_eq!(labels(&engine.mention_candidates("sommerfest", "lange", 50).await), ["Paul Lange"]);
    assert!(engine.mention_candidates("sommerfest", "Lena heute", 50).await.is_empty());
    assert!(engine.mention_candidates("lena", "", 50).await.is_empty(), "nobody to mention in a one-to-one chat");
    let pick = |label: &str| {
        let c = all.iter().find(|c| c.label == label).unwrap();
        talk_core::MentionPick { label: c.label.clone(), mention_id: c.mention_id.clone() }
    };

    // What is in the chat already: everybody means the user too, a group does not say so.
    let page = engine.open_chat("sommerfest").await.unwrap();
    let find = |start: &str| page.messages.iter().find(|m| m.text.starts_with(start)).unwrap().clone();
    let everybody = find("@Alle Der Getränkewagen");
    assert_eq!(kinds(&everybody), [("@Alle".to_string(), "all", true)]);
    assert!(everybody.mentions_me);
    let helpers = find("@Helferkreis wer bringt");
    assert_eq!(kinds(&helpers), [("@Helferkreis".to_string(), "group", false)]);
    assert!(!helpers.mentions_me);
    let two = find("@Paul Lange und @Orga-Team:");
    assert_eq!(kinds(&two), [("@Paul Lange".to_string(), "user", false), ("@Orga-Team".to_string(), "team", false)]);
    assert_eq!(two.mentions[0].id, "paul");
    assert!(page.unread_mentions.is_empty(), "all read");
    assert!(page.messages.iter().filter(|m| m.kind == "system").all(|m| !m.text.contains('@') && m.mentions.is_empty()));

    // Sending: the waiting bubble shows names, the server gets its notation.
    let text = "@Paul Lange und @Helferkreis: @Alle bitte lesen.";
    let picks = vec![pick("Paul Lange"), pick("Helferkreis"), pick("Alle")];
    let pending = engine.enqueue_text("sommerfest", text, picks, None, "ref-mention");
    assert_eq!(pending.message.text, text);
    assert_eq!(
        kinds(&pending.message),
        [("@Paul Lange".to_string(), "user", false), ("@Helferkreis".to_string(), "group", false), ("@Alle".to_string(), "all", false)]
    );
    let sent = arrived(&sink, "@Paul Lange und @Helferkreis: @Alle bitte lesen.").await;
    assert!(sent.is_own && !sent.mentions_me && sent.mentions.len() == 3 && sent.mentions[2].is_self);
    let wire = client(&server).history("sommerfest", None, 3).await.unwrap().messages;
    let wire = wire.iter().find(|m| m.id == sent.id).unwrap();
    assert_eq!(wire.message, "{mention-user1} und {mention-group1}: {mention-call1} bitte lesen.");
    assert_eq!(wire.source_text(), r#"@"paul" und @"group/helfer kreis": @"all" bitte lesen."#);

    // Editing: the input field gets names and who they are; one more mention is added.
    let draft = engine.edit_source("sommerfest", sent.id).await.unwrap();
    assert_eq!(draft.text, text);
    assert_eq!(draft.mentions, [pick("Paul Lange"), pick("Helferkreis"), pick("Alle")]);
    let mut picks = draft.mentions.clone();
    picks.push(pick("Lena Hoffmann"));
    engine.edit_message("sommerfest", sent.id, "@Paul Lange, @Lena Hoffmann und @Helferkreis", &picks).await.unwrap();
    let new = updates(&sink, sent.id).pop().unwrap();
    assert_eq!(new.text, "@Paul Lange, @Lena Hoffmann und @Helferkreis");
    assert_eq!(new.mentions.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["paul", "lena", "group/helfer kreis"]);
    // Unchanged: nobody is asked.
    until("the edit over the live update", || updates(&sink, sent.id).len() >= 2).await;
    let hidden = client(&server).history("sommerfest", None, 50).await.unwrap().messages.len();
    engine.edit_message("sommerfest", sent.id, &new.text, &picks).await.unwrap();
    assert_eq!(client(&server).history("sommerfest", None, 50).await.unwrap().messages.len(), hidden);

    // The "@" button. A group the user belongs to: the server only says "mentioned".
    post(vec![("token", "sommerfest"), ("from", "sophie"), ("text", "@\"group/helfer kreis\" bitte Zeltheringe mitbringen")]).await;
    let group = arrived(&sink, "@Helferkreis bitte Zeltheringe mitbringen").await;
    assert!(!group.mentions_me);
    engine.refresh_rooms(false).await.unwrap();
    until("the group mention behind the read marker", || engine.cached_chat("sommerfest").unread_mentions == [group.id]).await;
    // By name: only that one counts now.
    post(vec![("token", "sommerfest"), ("from", "lena"), ("text", "@mira hast du den Schlüssel?")]).await;
    post(vec![("token", "sommerfest"), ("from", "lena"), ("text", "@all bis morgen!")]).await;
    let named = arrived(&sink, "@Mira Lange hast du den Schlüssel?").await;
    let everybody = arrived(&sink, "@Alle bis morgen!").await;
    assert!(named.mentions_me && named.mentions[0].is_self && everybody.mentions_me);
    until("the mentions behind the read marker", || {
        engine.cached_chat("sommerfest").unread_mentions == [named.id, everybody.id]
    })
    .await;
    // Read: nothing left.
    engine.mark_read("sommerfest", everybody.id).await.unwrap();
    assert!(engine.cached_chat("sommerfest").unread_mentions.is_empty());

    // A chat that was not open: its mention is found when it opens.
    post(vec![("token", "familie"), ("from", "hilde"), ("text", "@mira kommst du am Sonntag?")]).await;
    post(vec![("token", "familie"), ("from", "paul"), ("text", "Ich bringe Kuchen mit.")]).await;
    let page = engine.open_chat("familie").await.unwrap();
    let asked = page.messages.iter().find(|m| m.text == "@Mira Lange kommst du am Sonntag?").unwrap();
    assert_eq!(page.unread_mentions, [asked.id]);

    // Many unread messages: the cache starts with the newest only, the pages that are
    // loaded towards the read marker bring the older mention.
    post(vec![("token", "lesekreis"), ("from", "clara"), ("text", "@mira liest du mit?")]).await;
    for _ in 0..70 {
        post(vec![("token", "lesekreis"), ("from", "lena"), ("text", "Noch eine Seite.")]).await;
    }
    post(vec![("token", "lesekreis"), ("from", "sophie"), ("text", "@all Treffen ist am Dienstag.")]).await;
    engine.refresh_rooms(false).await.unwrap();
    let read = engine.rooms().into_iter().find(|r| r.token == "lesekreis").unwrap().last_read_message;
    let mut page = engine.open_chat("lesekreis").await.unwrap();
    let mut seen = page.unread_mentions.clone();
    let mut texts: Vec<(i64, String)> = page.messages.iter().map(|m| (m.id, m.text.clone())).collect();
    while page.has_more && page.oldest_id.is_some_and(|id| id > read) {
        page = engine.load_older("lesekreis", page.oldest_id.unwrap()).await.unwrap();
        seen.extend(page.unread_mentions.iter().copied());
        texts.extend(page.messages.iter().map(|m| (m.id, m.text.clone())));
    }
    seen.sort_unstable();
    seen.dedup();
    let id_of = |text: &str| texts.iter().find(|t| t.1 == text).unwrap().0;
    assert_eq!(seen, [id_of("@Mira Lange liest du mit?"), id_of("@Alle Treffen ist am Dienstag.")]);

    // A file with a caption: the mention goes along.
    let path = dir.join("plan.txt");
    std::fs::write(&path, b"Plan").unwrap();
    engine
        .enqueue_file(
            "sommerfest",
            &path.to_string_lossy(),
            "plan.txt",
            "text/plain",
            0,
            0,
            "@Lena Hoffmann der Plan",
            vec![pick("Lena Hoffmann")],
            None,
            "ref-caption",
        )
        .unwrap();
    let file = arrived(&sink, "@Lena Hoffmann der Plan").await;
    assert!(file.attachment.is_some());
    assert_eq!((file.mentions[0].label.as_str(), file.mentions[0].id.as_str()), ("@Lena Hoffmann", "lena"));

    // Tapping a mention: the chat with that person, when there is one.
    assert_eq!(engine.existing_direct("lena").as_deref(), Some("lena"));
    assert_eq!(engine.existing_direct("jonas"), None);

    engine.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}
