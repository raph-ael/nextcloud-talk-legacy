//! Shape checks with the clients' own wire types (talk-core), and an end-to-end run of
//! talk-core's client and sync engine against the server on a free local port.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::Value;
use talk_core::{Account, ChatMessage, ConversationTag, Engine, Event, EventSink, Participant, Platform, Reactions, Room, TalkClient};

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
    let app = crate::app_from(fixtures()).unwrap();
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
