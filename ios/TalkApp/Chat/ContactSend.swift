import Contacts
import ContactsUI
import SwiftUI

/// docs/attachments.md "Kontakt": the system's contact picker for one entry. It runs out of
/// process, so the app needs no access to the address book and asks for none; it only gets
/// the picked entry. Cancelling changes nothing.
@MainActor
enum ContactPicker {
    private static var delegate: Delegate?

    /// `onPick` gets nil when the picked entry cannot be read.
    static func present(onPick: @escaping (PickedContact?) -> Void) {
        guard let scene = UIApplication.shared.connectedScenes.first(where: { $0.activationState == .foregroundActive }) as? UIWindowScene
                ?? UIApplication.shared.connectedScenes.first as? UIWindowScene,
              var top = scene.keyWindow?.rootViewController else { return }
        while let next = top.presentedViewController { top = next }
        let d = Delegate(onPick: onPick)
        delegate = d
        let picker = CNContactPickerViewController()
        picker.delegate = d
        top.present(picker, animated: true)
    }

    private final class Delegate: NSObject, CNContactPickerDelegate {
        let onPick: (PickedContact?) -> Void
        init(onPick: @escaping (PickedContact?) -> Void) { self.onPick = onPick }

        func contactPicker(_ picker: CNContactPickerViewController, didSelect contact: CNContact) {
            let picked = PickedContact(contact)
            // The picker closes itself; the sheet follows once it is gone (a sheet presented
            // during that transition would not appear).
            Task { @MainActor [onPick] in
                var tries = 0
                while tries < 40, picker.viewIfLoaded?.window != nil || picker.isBeingDismissed {
                    tries += 1
                    try? await Task.sleep(for: .milliseconds(50))
                }
                onPick(picked)
                ContactPicker.delegate = nil
            }
        }

        func contactPickerDidCancel(_ picker: CNContactPickerViewController) {
            Task { @MainActor in ContactPicker.delegate = nil }
        }
    }
}

/// The part of an address book entry the sheet offers: name, organization and one line per
/// number, e-mail address and postal address. Photo, notes, birthday and everything else are
/// never read.
struct PickedContact: Identifiable {
    enum Field { case phone, email, address }

    struct Line: Identifiable {
        let id: Int
        let field: Field
        /// As shown in the sheet (an address on one line).
        let value: String
        /// "mobile", "home", "work" or empty.
        let kind: String
        let address: ContactAddress?
    }

    let id = UUID()
    /// As the address book shows it, but only a real name: empty when the book shows the
    /// organization, a number or the nickname in its place (docs/attachments.md "Name"), so
    /// an unticked number never ends up in the name.
    let name: String
    let givenName: String
    let familyName: String
    let organization: String
    let lines: [Line]

    /// nil when the entry cannot be read ("Kontakt konnte nicht gelesen werden").
    init?(_ c: CNContact) {
        let has = { (key: String) in c.isKeyAvailable(key) }
        let readable = [CNContactGivenNameKey, CNContactFamilyNameKey, CNContactOrganizationNameKey,
                        CNContactPhoneNumbersKey, CNContactEmailAddressesKey, CNContactPostalAddressesKey]
        guard readable.contains(where: has) else { return nil }
        givenName = has(CNContactGivenNameKey) ? c.givenName.trimmingCharacters(in: .whitespacesAndNewlines) : ""
        familyName = has(CNContactFamilyNameKey) ? c.familyName.trimmingCharacters(in: .whitespacesAndNewlines) : ""
        organization = has(CNContactOrganizationNameKey) ? c.organizationName.trimmingCharacters(in: .whitespacesAndNewlines) : ""
        let parts = [CNContactNamePrefixKey, CNContactGivenNameKey, CNContactMiddleNameKey, CNContactFamilyNameKey, CNContactNameSuffixKey]
        let personal = has(CNContactTypeKey) ? c.contactType == .person : true
        let named = parts.contains { has($0) && !((c.value(forKey: $0) as? String) ?? "").trimmingCharacters(in: .whitespaces).isEmpty }
        let fullName = CNContactFormatter.descriptorForRequiredKeys(for: .fullName)
        if personal, named, c.areKeysAvailable([fullName]) {
            name = (CNContactFormatter.string(from: c, style: .fullName) ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
        } else if personal, named {
            name = [givenName, familyName].filter { !$0.isEmpty }.joined(separator: " ")
        } else {
            name = ""
        }

        var lines: [Line] = []
        var seen = Set<String>()
        func add(_ field: Field, _ value: String, _ label: String?, _ address: ContactAddress? = nil) {
            let v = value.trimmingCharacters(in: .whitespacesAndNewlines)
            // Linked entries can bring the same value twice (docs/attachments.md "Doppelte"):
            // numbers without spaces and separators, e-mail addresses in any case.
            let same: String = switch field {
            case .phone: String(v.filter { !$0.isWhitespace && !"-/().".contains($0) })
            case .email: v.lowercased()
            case .address: v
            }
            guard !v.isEmpty, seen.insert("\(field)|\(same)").inserted else { return }
            lines.append(Line(id: lines.count, field: field, value: v, kind: Self.kind(label), address: address))
        }
        if has(CNContactPhoneNumbersKey) {
            for p in c.phoneNumbers { add(.phone, p.value.stringValue, p.label) }
        }
        if has(CNContactEmailAddressesKey) {
            for e in c.emailAddresses { add(.email, e.value as String, e.label) }
        }
        if has(CNContactPostalAddressesKey) {
            let formatter = CNPostalAddressFormatter()
            for a in c.postalAddresses {
                let p = a.value
                let shown = formatter.string(from: p)
                    .split(whereSeparator: \.isNewline)
                    .map { $0.trimmingCharacters(in: .whitespaces) }
                    .filter { !$0.isEmpty }
                    .joined(separator: ", ")
                let kind = Self.kind(a.label)
                add(.address, shown, a.label, ContactAddress(
                    kind: kind, street: p.street, city: p.city, region: p.state,
                    postalCode: p.postalCode, country: p.country
                ))
            }
        }
        self.lines = lines
    }

    /// docs/attachments.md "Kern-Vertrag": anything else (fax, pager, own labels) is empty.
    static func kind(_ label: String?) -> String {
        switch label {
        case CNLabelPhoneNumberMobile, CNLabelPhoneNumberiPhone: "mobile"
        case CNLabelHome: "home"
        case CNLabelWork: "work"
        default: ""
        }
    }

    static func kindLabel(_ kind: String) -> String {
        switch kind {
        case "mobile": "Mobil"
        case "home": "Privat"
        case "work": "Arbeit"
        default: ""
        }
    }

    /// The name that is sent, as the core falls back for it (docs/attachments.md "Name"):
    /// given and family name, the organization, else the first ticked number or e-mail
    /// address, "Kontakt" when nothing is left.
    func shownName(_ ticked: Set<Int>) -> String {
        if !name.isEmpty { return name }
        let full = [givenName, familyName].filter { !$0.isEmpty }.joined(separator: " ")
        if !full.isEmpty { return full }
        if !organization.isEmpty { return organization }
        let chosen = lines.filter { ticked.contains($0.id) }
        return (chosen.first { $0.field == .phone } ?? chosen.first { $0.field == .email })?.value ?? "Kontakt"
    }

    /// What goes out: the name, the organization and the ticked lines.
    func card(_ ticked: Set<Int>) -> ContactCard {
        let chosen = lines.filter { ticked.contains($0.id) }
        return ContactCard(
            name: name, givenName: givenName, familyName: familyName, organization: organization,
            phones: chosen.filter { $0.field == .phone }.map { ContactValue(kind: $0.kind, value: $0.value) },
            emails: chosen.filter { $0.field == .email }.map { ContactValue(kind: $0.kind, value: $0.value) },
            addresses: chosen.compactMap(\.address)
        )
    }
}

/// docs/attachments.md "Blatt": avatar with initials, name and organization, then one ticked
/// line per number, e-mail address and postal address. Tapping a line takes its tick away;
/// without a ticked line the button is off.
struct ContactSendSheet: View {
    let contact: PickedContact
    let onSend: (ContactCard) -> Void
    let onClose: () -> Void

    @State private var ticked: Set<Int>

    init(contact: PickedContact, onSend: @escaping (ContactCard) -> Void, onClose: @escaping () -> Void) {
        self.contact = contact
        self.onSend = onSend
        self.onClose = onClose
        _ticked = State(initialValue: Set(contact.lines.map(\.id)))
    }

    var body: some View {
        VStack(spacing: 0) {
            ZStack {
                Text("Kontakt senden").font(.system(size: 17, weight: .semibold))
                HStack {
                    Spacer()
                    Button(action: onClose) {
                        Image(systemName: "xmark")
                            .font(.system(size: 15, weight: .semibold))
                            .foregroundStyle(Talk.muted)
                            .frame(width: 32, height: 32)
                            .background(Talk.chip, in: Circle())
                    }
                    .accessibilityLabel("Schließen")
                    .accessibilityIdentifier("contact-close")
                }
            }
            .padding(.horizontal, 16)
            .padding(.top, 16)
            .padding(.bottom, 10)
            ScrollView {
                VStack(spacing: 4) {
                    let shown = contact.shownName(ticked)
                    Avatar(name: shown, size: 64)
                        .padding(.bottom, 6)
                    Text(shown)
                        .font(.system(size: 18, weight: .semibold))
                        .multilineTextAlignment(.center)
                        .accessibilityIdentifier("contact-name")
                    if !contact.organization.isEmpty {
                        Text(contact.organization)
                            .font(.system(size: 14))
                            .foregroundStyle(Talk.muted)
                            .accessibilityIdentifier("contact-organization")
                    }
                }
                .frame(maxWidth: .infinity)
                .padding(.horizontal, 16)
                .padding(.bottom, 16)
                if contact.lines.isEmpty {
                    Text("Dieser Kontakt hat keine Nummer und keine Adresse")
                        .font(.system(size: 15))
                        .foregroundStyle(Talk.muted)
                        .multilineTextAlignment(.center)
                        .padding(.horizontal, 24)
                        .padding(.top, 8)
                        .accessibilityIdentifier("contact-empty")
                } else {
                    VStack(spacing: 0) {
                        ForEach(contact.lines) { line in row(line) }
                    }
                }
            }
            Button {
                onSend(contact.card(ticked))
            } label: {
                Text("Kontakt senden")
                    .font(.system(size: 16, weight: .semibold))
                    .foregroundStyle(.white)
                    .frame(maxWidth: .infinity, minHeight: 48)
                    .background(Talk.accent.opacity(ticked.isEmpty ? 0.4 : 1), in: Capsule())
            }
            .buttonStyle(.plain)
            .disabled(ticked.isEmpty)
            .accessibilityIdentifier("contact-send")
            .padding(.horizontal, 16)
            .padding(.vertical, 12)
        }
        .foregroundStyle(Talk.onSurface)
        .background(Talk.background)
    }

    private func row(_ line: PickedContact.Line) -> some View {
        let on = ticked.contains(line.id)
        let kind = PickedContact.kindLabel(line.kind)
        return Button {
            if on { ticked.remove(line.id) } else { ticked.insert(line.id) }
        } label: {
            HStack(spacing: 14) {
                Image(systemName: on ? "checkmark.circle.fill" : "circle")
                    .font(.system(size: 22))
                    .foregroundStyle(on ? Talk.accent : Talk.muted)
                VStack(alignment: .leading, spacing: 1) {
                    Text(line.value).font(.system(size: 16)).lineLimit(3)
                    if !kind.isEmpty {
                        Text(kind).font(.system(size: 13)).foregroundStyle(Talk.muted)
                    }
                }
                Spacer(minLength: 0)
            }
            .padding(.horizontal, 20)
            .padding(.vertical, 10)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(kind.isEmpty ? line.value : "\(line.value), \(kind)")
        .accessibilityAddTraits(on ? [.isButton, .isSelected] : .isButton)
        .accessibilityIdentifier("contact-line")
    }
}

/// docs/attachments.md "Kontaktkachel": initials avatar, the name (two lines at most) and
/// "Kontakt" below, never a photo. Pending like a file: the ring in place of the avatar, a
/// tap on a failed or waiting one sends it again. Delivered: a tap loads the vCard and hands
/// it to the preview, which offers "Neuen Kontakt erstellen"; the app writes nothing to the
/// address book.
struct ContactRow: View {
    let m: UiMessage
    let attachment: FileAttachment
    let textColor: Color
    let onRetry: (MessageView) -> Void

    var body: some View {
        let a = attachment
        let opener = FileOpener.shared
        let loading = opener.isLoading(a)
        let pending = m.status == .sending || m.status == .waiting
        HStack(spacing: 10) {
            ZStack {
                if pending || loading {
                    Circle().fill(textColor.opacity(0.12))
                    if pending && (0..<0.999).contains(m.progress) {
                        Circle().stroke(textColor.opacity(0.25), lineWidth: 3).padding(5)
                        Circle().trim(from: 0, to: CGFloat(max(0.03, m.progress)))
                            .stroke(textColor, style: StrokeStyle(lineWidth: 3, lineCap: .round))
                            .rotationEffect(.degrees(-90))
                            .padding(5)
                    } else {
                        ProgressView().tint(textColor)
                    }
                } else {
                    Avatar(name: a.contactName, size: 40)
                }
            }
            .frame(width: 40, height: 40)
            VStack(alignment: .leading, spacing: 1) {
                Text(a.contactName).font(.system(size: 15, weight: .medium)).lineLimit(2)
                Text("Kontakt").font(.system(size: 12)).opacity(0.75)
            }
        }
        .foregroundStyle(textColor)
        .padding(.vertical, 4)
        .contentShape(Rectangle())
        // A tap gesture, not a Button: a Button would swallow the long press of the list
        // that opens the message menu.
        .onTapGesture {
            switch m.status {
            case .failed, .waiting: onRetry(m.msg)
            case .sending: break
            case nil: opener.open(a)
            }
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Kontakt: \(a.contactName)")
        .accessibilityValue(loading ? "Wird geladen" : "")
        .accessibilityAddTraits(.isButton)
        .accessibilityIdentifier("contact-row")
    }
}
