//! Contacts shared into a conversation (docs/attachments.md "Kontakt").
//!
//! Talk has no message type for them: a contact is a vCard file, and the server names
//! the person in the file parameter (`contact-name`). So sending is a file upload, and a
//! received contact opens like any file, with the system's contacts app.

use serde::{Deserialize, Serialize};

/// MIME types that make a file a contact. Nextcloud says `text/vcard`.
pub fn is_vcard(mimetype: &str) -> bool {
    matches!(mimetype.trim().to_ascii_lowercase().as_str(), "text/vcard" | "text/x-vcard" | "text/directory")
}

/// What the chat shows for a contact file: the name the server read from the card, else
/// the file name without `.vcf`. Never empty.
pub fn display_name(contact_name: &str, file_name: &str) -> String {
    let stem = file_name.trim();
    let stem = match stem.len().checked_sub(4) {
        Some(at) if stem.is_char_boundary(at) && stem[at..].eq_ignore_ascii_case(".vcf") => &stem[..at],
        _ => stem,
    };
    [contact_name.trim(), stem.trim()]
        .into_iter()
        .find(|s| !s.is_empty())
        .unwrap_or("Kontakt")
        .to_string()
}

/// A phone number or an e-mail address. `kind`: "mobile", "home", "work" or empty.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ContactValue {
    pub kind: String,
    pub value: String,
}

/// A postal address. `kind`: "home", "work" or empty.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ContactAddress {
    pub kind: String,
    pub street: String,
    pub city: String,
    pub region: String,
    pub postal_code: String,
    pub country: String,
}

/// The part of an address book entry that is sent: what the user left ticked.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ContactCard {
    /// The name as the address book shows it.
    pub name: String,
    pub given_name: String,
    pub family_name: String,
    pub organization: String,
    pub phones: Vec<ContactValue>,
    pub emails: Vec<ContactValue>,
    pub addresses: Vec<ContactAddress>,
}

impl ContactCard {
    /// The name for the chat and the card: the display name, else given and family name,
    /// the organization, the first number or address. "Kontakt" if there is nothing.
    pub fn display_name(&self) -> String {
        let full = format!("{} {}", self.given_name.trim(), self.family_name.trim());
        let first = |values: &[ContactValue]| values.iter().map(|v| v.value.trim().to_string()).find(|v| !v.is_empty());
        [self.name.trim().to_string(), full.trim().to_string(), self.organization.trim().to_string()]
            .into_iter()
            .find(|s| !s.is_empty())
            .or_else(|| first(&self.phones))
            .or_else(|| first(&self.emails))
            .unwrap_or_else(|| "Kontakt".into())
    }

    /// The card as vCard 3.0 (RFC 2426), which every address book reads. Empty values are
    /// left out.
    pub fn vcard(&self) -> String {
        let mut lines = vec!["BEGIN:VCARD".to_string(), "VERSION:3.0".to_string()];
        let (given, family) = (self.given_name.trim(), self.family_name.trim());
        let name = self.display_name();
        // N is required. Without name parts the display name stands as the family name.
        let n = match given.is_empty() && family.is_empty() {
            true => format!("{};;;;", escape(&name)),
            false => format!("{};{};;;", escape(family), escape(given)),
        };
        lines.push(format!("N:{n}"));
        lines.push(format!("FN:{}", escape(&name)));
        if !self.organization.trim().is_empty() {
            lines.push(format!("ORG:{}", escape(self.organization.trim())));
        }
        for p in self.phones.iter().filter(|p| !p.value.trim().is_empty()) {
            let kind = match p.kind.as_str() {
                "mobile" => ";TYPE=CELL",
                "home" => ";TYPE=HOME",
                "work" => ";TYPE=WORK",
                _ => "",
            };
            lines.push(format!("TEL{kind}:{}", escape(p.value.trim())));
        }
        for e in self.emails.iter().filter(|e| !e.value.trim().is_empty()) {
            let kind = match e.kind.as_str() {
                "home" => ";TYPE=INTERNET,HOME",
                "work" => ";TYPE=INTERNET,WORK",
                _ => ";TYPE=INTERNET",
            };
            lines.push(format!("EMAIL{kind}:{}", escape(e.value.trim())));
        }
        for a in &self.addresses {
            let parts = [&a.street, &a.city, &a.region, &a.postal_code, &a.country].map(|p| escape(p.trim()));
            if parts.iter().all(String::is_empty) {
                continue;
            }
            let kind = match a.kind.as_str() {
                "home" => ";TYPE=HOME",
                "work" => ";TYPE=WORK",
                _ => "",
            };
            // Post office box and extended address stay empty.
            lines.push(format!("ADR{kind}:;;{}", parts.join(";")));
        }
        lines.push("END:VCARD".to_string());
        lines.iter().map(|l| fold(l)).collect::<Vec<_>>().join("\r\n") + "\r\n"
    }
}

/// Escapes a text value: backslash, comma, semicolon and line breaks.
fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            ',' => out.push_str("\\,"),
            ';' => out.push_str("\\;"),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

/// Folds a line at 75 bytes (RFC 2425): a line break and a space, never inside a character.
fn fold(line: &str) -> String {
    let mut out = String::with_capacity(line.len() + line.len() / 70 * 3);
    let mut width = 0;
    for c in line.chars() {
        if width + c.len_utf8() > 75 {
            out.push_str("\r\n ");
            width = 1;
        }
        out.push(c);
        width += c.len_utf8();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(kind: &str, value: &str) -> ContactValue {
        ContactValue { kind: kind.into(), value: value.into() }
    }

    #[test]
    fn a_card_is_a_vcard_every_address_book_reads() {
        let card = ContactCard {
            name: "Hanna Lindqvist".into(),
            given_name: "Hanna".into(),
            family_name: "Lindqvist".into(),
            organization: "Gärtnerei Lindqvist; Söhne".into(),
            phones: vec![value("mobile", " +49 30 23125 117 "), value("", "030 23125 118"), value("work", "  ")],
            emails: vec![value("work", "hanna@example.org")],
            addresses: vec![
                ContactAddress {
                    kind: "home".into(),
                    street: "Am Hof 3,\nHinterhaus".into(),
                    city: "Berlin".into(),
                    postal_code: "10249".into(),
                    country: "Deutschland".into(),
                    ..Default::default()
                },
                ContactAddress::default(),
            ],
        };
        assert_eq!(
            card.vcard(),
            "BEGIN:VCARD\r\nVERSION:3.0\r\nN:Lindqvist;Hanna;;;\r\nFN:Hanna Lindqvist\r\n\
             ORG:Gärtnerei Lindqvist\\; Söhne\r\nTEL;TYPE=CELL:+49 30 23125 117\r\nTEL:030 23125 118\r\n\
             EMAIL;TYPE=INTERNET,WORK:hanna@example.org\r\n\
             ADR;TYPE=HOME:;;Am Hof 3\\,\\nHinterhaus;Berlin;;10249;Deutschland\r\nEND:VCARD\r\n"
        );
    }

    #[test]
    fn the_name_is_never_empty() {
        let name = |card: ContactCard| card.display_name();
        assert_eq!(name(ContactCard { given_name: "Hanna".into(), ..Default::default() }), "Hanna");
        assert_eq!(name(ContactCard { organization: "Gärtnerei".into(), ..Default::default() }), "Gärtnerei");
        assert_eq!(name(ContactCard { phones: vec![value("", "030 23125 117")], ..Default::default() }), "030 23125 117");
        assert_eq!(name(ContactCard::default()), "Kontakt");
        // A card with a number only still has the required name lines.
        let card = ContactCard { phones: vec![value("", "030 23125 117")], ..Default::default() };
        assert!(card.vcard().contains("N:030 23125 117;;;;\r\nFN:030 23125 117\r\n"));

        assert_eq!(display_name(" Hanna Lindqvist ", "x.vcf"), "Hanna Lindqvist");
        assert_eq!(display_name("", "Hanna Lindqvist.VCF"), "Hanna Lindqvist");
        assert_eq!(display_name("", "Ünal.vcf"), "Ünal");
        assert_eq!(display_name("", ".vcf"), "Kontakt");
        assert_eq!(display_name("", "vcf"), "vcf");
        assert!(is_vcard("text/vcard") && is_vcard("Text/X-VCard") && !is_vcard("text/plain"));
    }

    #[test]
    fn long_lines_fold_between_characters() {
        let card = ContactCard { name: "Ä".repeat(60), ..Default::default() };
        let text = card.vcard();
        for line in text.split("\r\n") {
            assert!(line.len() <= 75, "{} bytes", line.len());
        }
        // Unfolded it is the name again.
        assert!(text.replace("\r\n ", "").contains(&format!("FN:{}\r\n", "Ä".repeat(60))));
    }
}
