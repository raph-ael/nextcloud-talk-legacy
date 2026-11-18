package org.wolkenfunk.app.ui

import android.Manifest
import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.provider.ContactsContract.CommonDataKinds.Email
import android.provider.ContactsContract.CommonDataKinds.Organization
import android.provider.ContactsContract.CommonDataKinds.Phone
import android.provider.ContactsContract.CommonDataKinds.StructuredName
import android.provider.ContactsContract.CommonDataKinds.StructuredPostal
import android.provider.ContactsContract.Contacts
import android.provider.ContactsContract.Data
import android.provider.ContactsContract.DisplayNameSources
import android.provider.Settings
import android.telephony.PhoneNumberUtils
import android.widget.Toast
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.Checkbox
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.content.ContextCompat
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LifecycleEventEffect
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import uniffi.talk_ffi.ContactAddress
import uniffi.talk_ffi.ContactCard
import uniffi.talk_ffi.ContactValue

internal sealed interface ContactStage {
    /** The app may not read contacts. */
    data object Denied : ContactStage

    /** The entry picked in the system's chooser. */
    data class Picked(val uri: Uri) : ContactStage
}

/** „Kontakt“ of the attachment menu, see [rememberContactSend]. */
@Stable
class ContactSend internal constructor() {
    internal var stage by mutableStateOf<ContactStage?>(null)
    internal var start: () -> Unit = {}

    /** The button was tapped: the permission question the first time, then the system's chooser. */
    fun open() = start()
}

private fun mayReadContacts(context: Context): Boolean =
    ContextCompat.checkSelfPermission(context, Manifest.permission.READ_CONTACTS) == PackageManager.PERMISSION_GRANTED

/**
 * Sending a contact (docs/attachments.md „Kontakt“): asks for the permission before the
 * chooser of the system opens, for one entry. The app reads only that entry and only then,
 * never the address book as a whole. [key] is the chat: another one starts over. Draw
 * [ContactSendSheet] for what comes after the choice.
 */
@Composable
fun rememberContactSend(key: Any?): ContactSend {
    val context = LocalContext.current
    val state = remember(key) { ContactSend() }
    val pick = rememberLauncherForActivityResult(ActivityResultContracts.PickContact()) { uri ->
        if (uri != null) state.stage = ContactStage.Picked(uri)
    }
    val choose = {
        try {
            pick.launch(null)
        } catch (e: ActivityNotFoundException) {
            Toast.makeText(context, "Keine Kontakte-App gefunden", Toast.LENGTH_SHORT).show()
        }
    }
    val ask = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { ok ->
        if (ok) choose() else state.stage = ContactStage.Denied
    }
    state.start = { if (mayReadContacts(context)) choose() else ask.launch(Manifest.permission.READ_CONTACTS) }
    return state
}

/** The sheets of [state]: „not allowed“ with the way into the settings, or what goes along. */
@Composable
fun ContactSendSheet(state: ContactSend, onSend: (ContactCard) -> Unit) {
    val context = LocalContext.current
    when (val stage = state.stage) {
        null -> Unit
        ContactStage.Denied -> DeniedSheet(
            onAllowed = {
                state.stage = null
                state.open()
            },
            onDismiss = { state.stage = null },
        )
        is ContactStage.Picked -> {
            val entry by produceState<AddressBookEntry?>(null, stage.uri) {
                val read = withContext(Dispatchers.IO) { runCatching { readContact(context, stage.uri) }.getOrNull() }
                if (read == null) {
                    Toast.makeText(context, "Kontakt konnte nicht gelesen werden", Toast.LENGTH_SHORT).show()
                    state.stage = null
                } else {
                    value = read
                }
            }
            entry?.let {
                PickedSheet(
                    it,
                    onSend = { card ->
                        state.stage = null
                        onSend(card)
                    },
                    onDismiss = { state.stage = null },
                )
            }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun DeniedSheet(onAllowed: () -> Unit, onDismiss: () -> Unit) {
    val context = LocalContext.current
    // Back from the settings with the permission: on to the chooser.
    LifecycleEventEffect(Lifecycle.Event.ON_RESUME) { if (mayReadContacts(context)) onAllowed() }
    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)) {
        Column(Modifier.fillMaxWidth().padding(start = 20.dp, end = 20.dp, bottom = 20.dp)) {
            Text("Kontakt senden", style = ListType.section, modifier = Modifier.padding(bottom = 14.dp).semantics { heading() })
            Column(Modifier.fillMaxWidth().padding(vertical = 20.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                Text(
                    "Wolkenfunk darf die Kontakte nicht lesen",
                    color = MaterialTheme.colorScheme.onSurface,
                    fontSize = 15.sp,
                    textAlign = TextAlign.Center,
                )
                TextButton(
                    onClick = {
                        runCatching {
                            context.startActivity(
                                Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.fromParts("package", context.packageName, null)),
                            )
                        }
                    },
                ) { Text("Einstellungen öffnen") }
            }
        }
    }
}

/** One line with a tick: a number, an e-mail address or a postal address. */
private class Line(val text: String, val kind: String)

private fun kindLabel(kind: String): String = when (kind) {
    "mobile" -> "Mobil"
    "home" -> "Privat"
    "work" -> "Arbeit"
    else -> ""
}

/** The address in one line: „Am Hof 3, 10249 Berlin, Deutschland“. */
private fun ContactAddress.oneLine(): String =
    listOf(street, listOf(postalCode, city).filter { it.isNotBlank() }.joinToString(" ") { it.trim() }, region, country)
        .map { it.trim().replace(Regex("\\s*\\n\\s*"), ", ") }
        .filter { it.isNotEmpty() }
        .joinToString(", ")

/** The name the core gives the card (`ContactCard::display_name`): shown here before it goes out. */
private fun ContactCard.shownName(): String =
    sequenceOf(name, "$givenName $familyName", organization)
        .plus(phones.asSequence().map { it.value })
        .plus(emails.asSequence().map { it.value })
        .map { it.trim() }
        .firstOrNull { it.isNotEmpty() } ?: "Kontakt"

/**
 * „Kontakt senden“: avatar, name and organization, below each number, e-mail address and
 * postal address with its tick. Name, organization and the ticked lines go along, nothing else.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun PickedSheet(entry: AddressBookEntry, onSend: (ContactCard) -> Unit, onDismiss: () -> Unit) {
    val colors = LocalTalkColors.current
    val lines = remember(entry) {
        entry.phones.map { Line(it.value, it.kind) } + entry.emails.map { Line(it.value, it.kind) } +
            entry.addresses.map { Line(it.oneLine(), it.kind) }
    }
    val ticked = remember(entry) { mutableStateListOf<Boolean>().apply { repeat(lines.size) { add(true) } } }
    val card = entry.card { ticked[it] }
    val name = card.shownName()

    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)) {
        Column(Modifier.fillMaxWidth().padding(bottom = 20.dp)) {
            Text(
                "Kontakt senden",
                style = ListType.section,
                modifier = Modifier.padding(start = 20.dp, end = 20.dp, bottom = 14.dp).semantics { heading() },
            )
            Row(Modifier.padding(start = 20.dp, end = 20.dp, bottom = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                Avatar(name, 48.dp)
                Spacer(Modifier.width(14.dp))
                Column {
                    Text(name, fontSize = 17.sp, fontWeight = FontWeight.Medium, maxLines = 2, overflow = TextOverflow.Ellipsis)
                    if (entry.organization.isNotEmpty() && entry.organization != name) {
                        Text(entry.organization, color = colors.muted, fontSize = 14.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    }
                }
            }
            Column(Modifier.weight(1f, fill = false).verticalScroll(rememberScrollState())) {
                for ((i, line) in lines.withIndex()) {
                    Row(
                        Modifier
                            .fillMaxWidth()
                            .heightIn(min = 56.dp)
                            .toggleable(value = ticked[i], role = Role.Checkbox) { ticked[i] = it }
                            .padding(horizontal = 20.dp, vertical = 6.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Checkbox(checked = ticked[i], onCheckedChange = null)
                        Spacer(Modifier.width(16.dp))
                        Column(Modifier.weight(1f)) {
                            Text(line.text, style = ListType.title, maxLines = 3, overflow = TextOverflow.Ellipsis)
                            kindLabel(line.kind).takeIf { it.isNotEmpty() }?.let { Text(it, color = colors.muted, fontSize = 13.sp) }
                        }
                    }
                }
                if (lines.isEmpty()) {
                    Text(
                        "Dieser Kontakt hat keine Nummer und keine Adresse",
                        color = colors.muted,
                        fontSize = 15.sp,
                        textAlign = TextAlign.Center,
                        modifier = Modifier.fillMaxWidth().padding(horizontal = 20.dp, vertical = 20.dp),
                    )
                }
            }
            Button(
                onClick = { onSend(card) },
                enabled = ticked.any { it },
                modifier = Modifier.fillMaxWidth().padding(start = 20.dp, end = 20.dp, top = 12.dp),
            ) {
                Text("Kontakt senden")
            }
        }
    }
}

/**
 * What the app reads of the picked entry: the name, the organization, numbers, e-mail
 * addresses and postal addresses. No photo, notes, birthday, nickname or anything else.
 */
private class AddressBookEntry(
    /** The name as the address book shows it; empty when the entry has no name of its own. */
    val name: String,
    val givenName: String,
    val familyName: String,
    val organization: String,
    val phones: List<ContactValue>,
    val emails: List<ContactValue>,
    val addresses: List<ContactAddress>,
) {
    /** The card with the lines [ticked] says (counted over numbers, e-mail addresses, postal addresses). */
    fun card(ticked: (Int) -> Boolean) = ContactCard(
        name = name,
        givenName = givenName,
        familyName = familyName,
        organization = organization,
        phones = phones.filterIndexed { i, _ -> ticked(i) },
        emails = emails.filterIndexed { i, _ -> ticked(phones.size + i) },
        addresses = addresses.filterIndexed { i, _ -> ticked(phones.size + emails.size + i) },
    )
}

/**
 * Reads the entry behind [uri] (from the system's chooser), or `null` if it is gone. Linked
 * entries bring the same value several times: each is kept once. Blocking.
 */
private fun readContact(context: Context, uri: Uri): AddressBookEntry? {
    val resolver = context.contentResolver
    var shown = ""
    var source = DisplayNameSources.UNDEFINED
    var nameRow = -1L
    resolver.query(uri, arrayOf(Contacts.DISPLAY_NAME, Contacts.DISPLAY_NAME_SOURCE, Contacts.NAME_RAW_CONTACT_ID), null, null, null)
        ?.use { c ->
            if (!c.moveToFirst()) return null
            shown = c.getString(0).orEmpty().trim()
            source = c.getInt(1)
            nameRow = if (c.isNull(2)) -1L else c.getLong(2)
        } ?: return null
    // Without a name the address book shows the organization, a number or the nickname in its
    // place. That is not the name: the core picks one from what is sent.
    val named = source >= DisplayNameSources.STRUCTURED_PHONETIC_NAME

    var given = ""
    var family = ""
    var organization = ""
    val phones = LinkedHashMap<String, ContactValue>()
    val emails = LinkedHashMap<String, ContactValue>()
    val addresses = LinkedHashMap<String, ContactAddress>()
    // A value met twice keeps its first place; a kind fills in where the first had none.
    fun MutableMap<String, ContactValue>.keep(key: String, kind: String, value: String) {
        val old = this[key]
        if (old == null) this[key] = ContactValue(kind, value) else if (old.kind.isEmpty() && kind.isNotEmpty()) this[key] = ContactValue(kind, old.value)
    }

    val columns = arrayOf(
        Data.MIMETYPE, Data.RAW_CONTACT_ID, Data.DATA1, Data.DATA2, Data.DATA3,
        StructuredPostal.STREET, StructuredPostal.CITY, StructuredPostal.REGION, StructuredPostal.POSTCODE, StructuredPostal.COUNTRY,
    )
    resolver.query(Uri.withAppendedPath(uri, Contacts.Data.CONTENT_DIRECTORY), columns, null, null, Data._ID)?.use { c ->
        fun text(column: String): String = c.getColumnIndex(column).let { if (it < 0 || c.isNull(it)) "" else c.getString(it).orEmpty().trim() }
        fun type(): Int = text(Data.DATA2).toIntOrNull() ?: 0
        while (c.moveToNext()) {
            when (text(Data.MIMETYPE)) {
                StructuredName.CONTENT_ITEM_TYPE -> {
                    val own = text(Data.RAW_CONTACT_ID).toLongOrNull() == nameRow
                    if (named && (own || (given.isEmpty() && family.isEmpty()))) {
                        val g = text(StructuredName.GIVEN_NAME)
                        val f = text(StructuredName.FAMILY_NAME)
                        if (g.isNotEmpty() || f.isNotEmpty()) {
                            given = g
                            family = f
                        }
                    }
                }
                Organization.CONTENT_ITEM_TYPE -> if (organization.isEmpty()) organization = text(Organization.COMPANY)
                Phone.CONTENT_ITEM_TYPE -> {
                    val number = text(Phone.NUMBER)
                    val kind = when (type()) {
                        Phone.TYPE_MOBILE -> "mobile"
                        Phone.TYPE_HOME -> "home"
                        Phone.TYPE_WORK -> "work"
                        else -> ""
                    }
                    if (number.isNotEmpty()) phones.keep(PhoneNumberUtils.normalizeNumber(number).ifEmpty { number }, kind, number)
                }
                Email.CONTENT_ITEM_TYPE -> {
                    val address = text(Email.ADDRESS)
                    val kind = when (type()) {
                        Email.TYPE_MOBILE -> "mobile"
                        Email.TYPE_HOME -> "home"
                        Email.TYPE_WORK -> "work"
                        else -> ""
                    }
                    if (address.isNotEmpty()) emails.keep(address.lowercase(), kind, address)
                }
                StructuredPostal.CONTENT_ITEM_TYPE -> {
                    val kind = when (type()) {
                        StructuredPostal.TYPE_HOME -> "home"
                        StructuredPostal.TYPE_WORK -> "work"
                        else -> ""
                    }
                    var a = ContactAddress(
                        kind,
                        text(StructuredPostal.STREET), text(StructuredPostal.CITY), text(StructuredPostal.REGION),
                        text(StructuredPostal.POSTCODE), text(StructuredPostal.COUNTRY),
                    )
                    // Written as one text only: it stands as the street.
                    if (a.oneLine().isEmpty()) a = a.copy(street = text(StructuredPostal.FORMATTED_ADDRESS))
                    val key = a.oneLine().lowercase()
                    val old = addresses[key]
                    if (key.isNotEmpty() && (old == null || (old.kind.isEmpty() && kind.isNotEmpty()))) addresses[key] = old?.copy(kind = kind) ?: a
                }
            }
        }
    } ?: return null
    return AddressBookEntry(
        name = if (named) shown else "",
        givenName = given,
        familyName = family,
        organization = organization,
        phones = phones.values.toList(),
        emails = emails.values.toList(),
        addresses = addresses.values.toList(),
    )
}
