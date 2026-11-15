//! "There is a new message in conversation X": which conversations deserve a
//! notification (docs/notifications.md). One place for the rules, so desktop (which has
//! no push) and the mobile fallbacks agree.
//!
//! The room list is all the core sees of a conversation that is not open. Comparing
//! unread counts between two lists is not enough: a count also rises with "mark as
//! unread", and it can fall although new messages arrived (read elsewhere, then new
//! ones). So the core remembers per conversation the newest message it has seen, stores
//! that, and reports a conversation when a newer message from somebody else is there.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::engine::{MessagePreview, effective_notification_level};
use crate::models::{ChatMessage, Room, RoomType};

/// One conversation with something new, for a notification.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageNotice {
    pub token: String,
    pub room_name: String,
    pub is_group: bool,
    /// The user was mentioned (always true at "Nur Erwähnungen").
    pub mention: bool,
    pub unread: i64,
    /// The newest message as in the conversation list; `None` when the server does not
    /// show one (conversations marked as sensitive): "Neue Nachricht".
    pub message: Option<MessagePreview>,
}

/// What was last seen of a conversation.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
struct Mark {
    /// Id of the newest message (the activity time for conversations without one).
    stamp: i64,
    unread: i64,
}

fn stamp(room: &Room) -> i64 {
    room.last_message.as_ref().map_or(room.last_activity, |m| m.id)
}

/// Would a new message of this conversation be announced at all (docs/settings.md).
fn counts(room: &Room) -> bool {
    !room.is_archived
        && room.unread_messages > 0
        && match effective_notification_level(room.notification_level, room.room_type) {
            1 => true,
            2 => room.unread_mention || room.unread_mention_direct,
            _ => false,
        }
}

pub(crate) fn is_group(room: &Room) -> bool {
    !matches!(room.room_type, RoomType::OneToOne | RoomType::FormerOneToOne | RoomType::NoteToSelf)
}

#[derive(Debug, Default, PartialEq)]
pub(crate) struct Scan {
    /// Conversations to announce, in the order they were given.
    pub notify: Vec<String>,
    /// Conversations whose notification is obsolete: read, archived or gone.
    pub cleared: Vec<String>,
    /// First comparison with the server in this run: what came in while the app was closed.
    pub since_start: bool,
    /// The marks changed and should be stored.
    pub dirty: bool,
}

pub(crate) struct Notices {
    marks: HashMap<String, Mark>,
    /// Conversations announced and not cleared since.
    shown: HashSet<String>,
    /// There is something to compare the server's list with. Without it (first start after
    /// signing in) the first list is the starting point, not a flood of notifications.
    baseline: bool,
    /// The server's list was not compared yet in this run.
    first: bool,
}

impl Notices {
    /// `stored`: what [`Self::store`] returned in an earlier run. Without it (first run of
    /// this version) the cached conversations of the last run are the starting point.
    pub(crate) fn load(stored: Option<String>, cached: &[Room]) -> Self {
        let marks: Option<HashMap<String, Mark>> = stored.and_then(|s| serde_json::from_str(&s).ok());
        let baseline = marks.is_some() || !cached.is_empty();
        let marks = marks.unwrap_or_else(|| {
            cached
                .iter()
                .map(|r| (r.token.clone(), Mark { stamp: stamp(r), unread: r.unread_messages }))
                .collect()
        });
        Self { marks, shown: HashSet::new(), baseline, first: true }
    }

    pub(crate) fn store(&self) -> String {
        serde_json::to_string(&self.marks).unwrap_or_default()
    }

    /// Conversations as the server sent them. `all`: they are the complete list, so every
    /// other conversation is gone. `own`: the message is the user's own. `quiet`: the
    /// user's status is "do not disturb" (only important conversations announce).
    pub(crate) fn server(&mut self, rooms: &[Room], all: bool, own: impl Fn(&ChatMessage) -> bool, quiet: bool) -> Scan {
        let mut scan = Scan { since_start: self.first, ..Scan::default() };
        for room in rooms {
            let now = Mark { stamp: stamp(room), unread: room.unread_messages };
            let before = self.marks.get(&room.token).copied();
            let newer = match before {
                Some(b) => now.stamp > b.stamp,
                // A conversation that was not there before: its first message.
                None => now.stamp > 0,
            };
            let last = room.last_message.as_ref();
            // A system message ("X joined") moves the stamp without anything to read:
            // then only a risen count says that a message came with it.
            let readable = last.is_none_or(|m| !m.is_system()) || before.is_none_or(|b| now.unread > b.unread);
            let announce = self.baseline
                && newer
                && counts(room)
                && readable
                && !last.is_some_and(|m| own(m) || m.silent)
                && (!quiet || room.is_important);
            if announce {
                self.shown.insert(room.token.clone());
                scan.notify.push(room.token.clone());
            } else if (room.unread_messages == 0 || room.is_archived) && self.shown.remove(&room.token) {
                scan.cleared.push(room.token.clone());
            }
            let kept = Mark { stamp: now.stamp.max(before.map_or(0, |b| b.stamp)), unread: now.unread };
            if before != Some(kept) {
                self.marks.insert(room.token.clone(), kept);
                scan.dirty = true;
            }
        }
        if all {
            let present: HashSet<&str> = rooms.iter().map(|r| r.token.as_str()).collect();
            let gone: Vec<String> = self.marks.keys().filter(|t| !present.contains(t.as_str())).cloned().collect();
            for token in gone {
                self.marks.remove(&token);
                scan.dirty = true;
                if self.shown.remove(&token) {
                    scan.cleared.push(token);
                }
            }
        }
        self.baseline = true;
        self.first = false;
        scan
    }

    /// A conversation changed here (read, sent, archived): nothing to announce, but its
    /// notification may be obsolete. The stamp stays, so a message that arrived in an open
    /// chat is still announced by the next list (the platform decides whether to show it).
    pub(crate) fn local(&mut self, room: &Room) -> Scan {
        let mut scan = Scan::default();
        if (room.unread_messages == 0 || room.is_archived) && self.shown.remove(&room.token) {
            scan.cleared.push(room.token.clone());
        }
        if let Some(mark) = self.marks.get_mut(&room.token)
            && mark.unread != room.unread_messages
        {
            mark.unread = room.unread_messages;
            scan.dirty = true;
        }
        scan
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn room(token: &str, kind: u8, last: Option<(i64, &str)>, unread: i64) -> Room {
        let mut r: Room = serde_json::from_value(serde_json::json!({
            "id": 1, "token": token, "type": kind, "name": token, "displayName": token,
            "unreadMessages": unread, "lastActivity": 1000,
        }))
        .unwrap();
        r.last_message = last.map(|(id, actor)| {
            serde_json::from_value(serde_json::json!({
                "id": id, "token": token, "actorType": "users", "actorId": actor,
                "actorDisplayName": actor, "timestamp": 1000 + id, "message": "hi",
            }))
            .unwrap()
        });
        r
    }

    fn own(m: &ChatMessage) -> bool {
        m.actor_id == "me"
    }

    fn started(rooms: &[Room]) -> Notices {
        let mut n = Notices::load(None, rooms);
        // The first list of the run, unchanged.
        assert_eq!(n.server(rooms, true, own, false).notify, Vec::<String>::new());
        n
    }

    #[test]
    fn a_newer_message_from_somebody_else_announces_once() {
        let mut n = started(&[room("a", 1, Some((10, "bob")), 0)]);
        let next = [room("a", 1, Some((11, "bob")), 1)];
        let scan = n.server(&next, false, own, false);
        assert_eq!(scan.notify, ["a"]);
        assert!(scan.dirty && !scan.since_start);
        // The same list again (every poll) says nothing new.
        assert_eq!(n.server(&next, false, own, false), Scan::default());
        // Own messages, and counts that rise without a message ("mark as unread"), never do.
        assert!(n.server(&[room("a", 1, Some((12, "me")), 0)], false, own, false).notify.is_empty());
        assert!(n.server(&[room("a", 1, Some((12, "me")), 1)], false, own, false).notify.is_empty());
    }

    #[test]
    fn read_elsewhere_then_new_ones_still_announce() {
        let mut n = started(&[room("a", 1, Some((10, "bob")), 5)]);
        // Between two polls: read on the phone, then two new messages. The count fell.
        assert_eq!(n.server(&[room("a", 1, Some((12, "bob")), 2)], false, own, false).notify, ["a"]);
    }

    #[test]
    fn a_new_conversation_announces_its_first_message() {
        let mut n = started(&[room("a", 1, Some((10, "bob")), 0)]);
        let list = [room("a", 1, Some((10, "bob")), 0), room("neu", 1, Some((20, "carla")), 1)];
        assert_eq!(n.server(&list, true, own, false).notify, ["neu"]);
    }

    #[test]
    fn signing_in_is_a_starting_point_and_a_restart_reports_what_came_meanwhile() {
        // Fresh account: no cache, no marks. Everything unread is old news.
        let mut fresh = Notices::load(None, &[]);
        let list = [room("a", 1, Some((10, "bob")), 3), room("g", 2, Some((7, "bob")), 9)];
        let scan = fresh.server(&list, true, own, false);
        assert!(scan.notify.is_empty() && scan.since_start && scan.dirty);
        let stored = fresh.store();

        // Next run: the stored marks are the starting point, the first list is `since_start`.
        let mut again = Notices::load(Some(stored), &[]);
        let scan = again.server(&[room("a", 1, Some((14, "bob")), 4), list[1].clone()], true, own, false);
        assert_eq!((scan.notify.as_slice(), scan.since_start), (&["a".to_string()][..], true));
        assert!(!again.server(&[room("a", 1, Some((15, "bob")), 5)], false, own, false).since_start);

        // First run of this version: the cached list of the last run stands in for marks.
        let mut upgraded = Notices::load(None, &list);
        assert_eq!(upgraded.server(&[room("a", 1, Some((11, "bob")), 4)], false, own, false).notify, ["a"]);
    }

    #[test]
    fn levels_archive_silent_and_do_not_disturb() {
        let before = [
            room("eins", 1, Some((1, "bob")), 0),
            room("gruppe", 2, Some((1, "bob")), 0),
            room("aus", 1, Some((1, "bob")), 0),
            room("archiv", 1, Some((1, "bob")), 0),
            room("still", 1, Some((1, "bob")), 0),
            room("wichtig", 1, Some((1, "bob")), 0),
        ];
        let after = || {
            let mut list: Vec<Room> = before
                .iter()
                .map(|r| room(&r.token, if r.token == "gruppe" { 2 } else { 1 }, Some((2, "bob")), 1))
                .collect();
            list[2].notification_level = 3;
            list[3].is_archived = true;
            list[4].last_message.as_mut().unwrap().silent = true;
            list[5].is_important = true;
            list
        };
        // A group is at "Nur Erwähnungen" by default: only a mention counts.
        let mut n = started(&before);
        assert_eq!(n.server(&after(), true, own, false).notify, ["eins", "wichtig"]);
        let mut mentioned = after();
        mentioned[1].unread_mention = true;
        mentioned[1].last_message.as_mut().unwrap().id = 3;
        assert_eq!(n.server(&mentioned, true, own, false).notify, ["gruppe"]);

        // "Do not disturb": only the conversation marked as important. What was held back
        // is not announced later either.
        let mut quiet = started(&before);
        assert_eq!(quiet.server(&after(), true, own, true).notify, ["wichtig"]);
        assert!(quiet.server(&after(), true, own, false).notify.is_empty());
    }

    #[test]
    fn a_system_message_alone_does_not_announce_old_unread() {
        let mut n = started(&[room("g", 1, Some((10, "bob")), 2)]);
        let mut joined = room("g", 1, Some((11, "bob")), 2);
        joined.last_message.as_mut().unwrap().system_message = "user_added".into();
        assert!(n.server(std::slice::from_ref(&joined), false, own, false).notify.is_empty());
        // With a message before it the count rose: announce.
        let mut left = room("g", 1, Some((13, "bob")), 3);
        left.last_message.as_mut().unwrap().system_message = "user_removed".into();
        assert_eq!(n.server(&[left], false, own, false).notify, ["g"]);
    }

    #[test]
    fn reading_archiving_and_leaving_clear() {
        let before = [room("a", 1, Some((1, "bob")), 0), room("b", 1, Some((1, "bob")), 0), room("c", 1, Some((1, "bob")), 0)];
        let mut n = started(&before);
        let news: Vec<Room> = before.iter().map(|r| room(&r.token, 1, Some((2, "bob")), 1)).collect();
        assert_eq!(n.server(&news, true, own, false).notify.len(), 3);

        // Read here: cleared at once, and only once.
        let read = room("a", 1, Some((2, "bob")), 0);
        assert_eq!(n.local(&read).cleared, ["a"]);
        assert!(n.local(&read).cleared.is_empty());
        // Read elsewhere: the next list says so. Left: gone from the complete list.
        let scan = n.server(&[room("b", 1, Some((2, "bob")), 0)], true, own, false);
        let mut cleared = scan.cleared.clone();
        cleared.sort();
        assert_eq!(cleared, ["b", "c"]);
        // A message that arrives while its chat is open is still announced by the list.
        let mut open = room("b", 1, Some((3, "bob")), 0);
        assert!(n.local(&open).cleared.is_empty());
        open.unread_messages = 1;
        assert_eq!(n.server(&[open], false, own, false).notify, ["b"]);
    }
}
