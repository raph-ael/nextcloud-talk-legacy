//! Shape checks with the clients' own wire types (talk-core), and an end-to-end run of
//! talk-core's client and sync engine against the server on a free local port.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::Value;
use talk_core::{
    Account, ChatMessage, ConnectionState, ConversationTag, Engine, Event, EventSink, Participant, Platform, Reactions, Room,
    TalkClient,
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
    engine.network_changed();
    until("online", || engine.connection_state() == Online).await;
    assert!(!engine.rooms().is_empty());
    assert_eq!(connections(&sink), [Offline, Online]);
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
