//! Several accounts side by side (docs/accounts.md). The core has no account list: one
//! [`crate::Engine`] is one account, and the platform keeps the list and the secrets.
//! Here are the pieces every platform needs the same way: telling accounts apart and
//! handing back an app password that will not be used.

use serde::Serialize;

use crate::client::{Account, Platform, TalkClient};
use crate::engine::RoomView;
use crate::error::Result;

/// A server address reduced to what identifies the server: no scheme, no trailing
/// slash, lower case. `https://Cloud.Example.org/` and `cloud.example.org` are the same.
pub fn server_key(server: &str) -> String {
    let s = server.trim().trim_end_matches('/');
    let s = s.strip_prefix("https://").or_else(|| s.strip_prefix("http://")).unwrap_or(s);
    s.trim_end_matches('/').to_lowercase()
}

/// Identifies an account across logins: the same user on the same server always gets the
/// same key, whatever the login name was (an e-mail address works as login name too).
pub fn account_key(server: &str, user_id: &str) -> String {
    format!("{user_id}@{}", server_key(server))
}

/// Who a fresh login belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountIdentity {
    pub user_id: String,
    pub display_name: String,
    /// [`account_key`]: equal to the key of an account in the list means "already signed in".
    pub key: String,
}

fn client(server: &str, login_name: &str, app_password: &str, platform: Platform) -> Result<TalkClient> {
    TalkClient::new(
        Account {
            server: server.to_string(),
            user: login_name.to_string(),
            app_password: app_password.to_string(),
        },
        platform,
    )
}

/// Asks the server who signed in, before the account is stored and started.
pub async fn resolve_account(
    server: &str,
    login_name: &str,
    app_password: &str,
    platform: Platform,
) -> Result<AccountIdentity> {
    let user = client(server, login_name, app_password, platform)?.current_user().await?;
    Ok(AccountIdentity {
        key: account_key(server, &user.id),
        display_name: if user.display_name.is_empty() { user.id.clone() } else { user.display_name },
        user_id: user.id,
    })
}

/// Hands an app password back to the server without a session, e.g. the one of a login
/// that turned out to be an account which is signed in already.
pub async fn revoke_app_password(server: &str, login_name: &str, app_password: &str, platform: Platform) -> Result<()> {
    client(server, login_name, app_password, platform)?.revoke_app_password().await
}

/// What an account adds to badges (docs/settings.md, "Stummgeschaltete Chats").
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BadgeView {
    /// Conversations with something new that counts.
    pub chats: u32,
    /// Unread messages in those conversations.
    pub messages: u32,
}

/// „Alle Nachrichten“ counts, „Nur Erwähnungen“ only with a mention, „Aus“ and archived
/// conversations never.
pub fn badge(rooms: &[RoomView]) -> BadgeView {
    let mut b = BadgeView::default();
    for r in rooms {
        let counts = !r.is_archived
            && r.unread_messages > 0
            && match r.notification_level {
                3 => false,
                2 => r.unread_mention,
                _ => true,
            };
        if counts {
            b.chats += 1;
            b.messages += u32::try_from(r.unread_messages).unwrap_or(u32::MAX).min(u32::MAX - b.messages);
        }
    }
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_keys_ignore_scheme_case_and_slash() {
        assert_eq!(server_key("https://Cloud.Example.org/"), "cloud.example.org");
        assert_eq!(server_key("cloud.example.org"), "cloud.example.org");
        assert_eq!(server_key(" http://host:8099/nc/ "), "host:8099/nc");
        assert_eq!(account_key("https://Cloud.Example.org", "Anna"), "Anna@cloud.example.org");
        assert_ne!(account_key("https://a.example", "anna"), account_key("https://b.example", "anna"));
    }
}
