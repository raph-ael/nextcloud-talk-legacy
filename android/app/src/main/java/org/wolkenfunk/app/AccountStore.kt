package org.wolkenfunk.app

import android.annotation.SuppressLint
import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import android.util.Log
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.security.KeyStore
import java.security.SecureRandom
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * One entry of the account list (docs/accounts.md „Kontoliste“). The app password and the
 * push device key are not in here; they live encrypted next to it, one value per account.
 */
data class AccountEntry(
    val id: String,
    val server: String,
    val loginName: String,
    val userId: String,
    val name: String,
    /** From `resolveAccount`; empty for the adopted account until its first start. */
    val key: String,
    /** Data folder of the runtime (absolute). */
    val dir: String,
)

/** An app password that could not be handed back when its account signed out (no network). */
data class PendingRevoke(val server: String, val loginName: String, val appPassword: String)

/** Reading a secret: present, surely absent, or not readable (keystore error). */
sealed interface Secret {
    data class Present(val value: String) : Secret
    data object Missing : Secret
    data object Unreadable : Secret
}

/**
 * The account list in the preferences `account`. Secrets are encrypted with an AES key that
 * never leaves the Android Keystore (alias `talk-account`, as before).
 *
 * A keystore error is never taken as „no account“: nothing is deleted, the adoption of the
 * old single account is not finished (docs/accounts.md „Lesefehler ist nicht kein Konto“).
 */
class AccountStore(private val context: Context) {
    private val prefs = context.getSharedPreferences("account", Context.MODE_PRIVATE)

    /**
     * First start with several accounts: the single account of earlier versions becomes the
     * first entry, with its data folder `talk/` and its push key. The ciphertexts are copied
     * unchanged; the old keys go only after the list was written.
     */
    @SuppressLint("ApplySharedPref")
    fun migrate() {
        if (prefs.contains(KEY_ACCOUNTS)) {
            if (LEGACY.any { prefs.contains(it) }) prefs.edit().apply { LEGACY.forEach { remove(it) } }.commit()
            return
        }
        val server = prefs.getString("server", null) ?: return
        val login = prefs.getString("login", null) ?: return
        val secret = prefs.getString("secret", null) ?: return
        val id = newId(emptySet())
        val entry = AccountEntry(
            id = id,
            server = server,
            loginName = login,
            // Filled from the session's cache once it exists (no network needed).
            userId = login,
            name = login,
            key = "",
            dir = File(context.noBackupFilesDir, "talk").path,
        )
        val edit = prefs.edit()
            .putString(KEY_ACCOUNTS, JSONArray().put(entry.toJson()).toString())
            .putString(KEY_ACTIVE, id)
            .putString(secretKey(id), secret)
        prefs.getString("push_key", null)?.let { edit.putString(pushKeyKey(id), it) }
        if (!edit.commit()) {
            Log.w(TAG, "adoption not written, trying again next start")
            return
        }
        prefs.edit().apply { LEGACY.forEach { remove(it) } }.commit()
    }

    /** All accounts in the order they signed in. */
    fun accounts(): List<AccountEntry> {
        val raw = prefs.getString(KEY_ACCOUNTS, null) ?: return emptyList()
        return runCatching {
            val arr = JSONArray(raw)
            (0 until arr.length()).map { arr.getJSONObject(it).toEntry() }
        }.getOrElse {
            Log.e(TAG, "account list unreadable", it)
            emptyList()
        }
    }

    fun activeId(): String? = prefs.getString(KEY_ACTIVE, null)

    fun setActive(id: String) {
        prefs.edit().putString(KEY_ACTIVE, id).apply()
    }

    /** A fresh local id, at least 8 hex characters, never one of [taken]. */
    fun newId(taken: Set<String> = accounts().map { it.id }.toSet()): String {
        val random = SecureRandom()
        while (true) {
            val bytes = ByteArray(8).also { random.nextBytes(it) }
            val id = bytes.joinToString("") { "%02x".format(it) }
            if (id !in taken) return id
        }
    }

    /** Stores a new account with its app password; `false` when it could not be written. */
    @SuppressLint("ApplySharedPref")
    fun add(entry: AccountEntry, appPassword: String): Boolean {
        val list = accounts().filter { it.id != entry.id } + entry
        return prefs.edit()
            .putString(KEY_ACCOUNTS, list.toJson())
            .putString(secretKey(entry.id), encrypt(appPassword))
            .putString(KEY_ACTIVE, entry.id)
            .commit()
    }

    /** Name, user id or key changed (after a start). */
    fun update(entry: AccountEntry) {
        val list = accounts().map { if (it.id == entry.id) entry else it }
        prefs.edit().putString(KEY_ACCOUNTS, list.toJson()).apply()
    }

    /** A new app password for an account that could not be read (signed in again). */
    @SuppressLint("ApplySharedPref")
    fun replacePassword(id: String, appPassword: String): Boolean =
        prefs.edit().putString(secretKey(id), encrypt(appPassword)).commit()

    /** Removes entry, app password and push key of [id]. */
    @SuppressLint("ApplySharedPref")
    fun remove(id: String) {
        val list = accounts().filter { it.id != id }
        val edit = prefs.edit()
            .putString(KEY_ACCOUNTS, list.toJson())
            .remove(secretKey(id))
            .remove(pushKeyKey(id))
        if (activeId() == id) {
            list.firstOrNull()?.let { edit.putString(KEY_ACTIVE, it.id) } ?: edit.remove(KEY_ACTIVE)
        }
        edit.commit()
    }

    fun password(id: String): Secret = read(secretKey(id))

    /** Push device key (PKCS#8 PEM) of [id]. */
    fun pushKey(id: String): Secret = read(pushKeyKey(id))

    @SuppressLint("ApplySharedPref")
    fun savePushKey(id: String, pem: String) {
        // commit: the server gets the public key right after; the private key must not be lost.
        prefs.edit().putString(pushKeyKey(id), encrypt(pem)).commit()
    }

    /**
     * App passwords still to hand back (docs/notifications.md „Abmelden ohne Netz“), encrypted
     * like the others. Not readable right now: empty, and nothing is overwritten.
     */
    fun pendingRevokes(): List<PendingRevoke> {
        val raw = (read(KEY_REVOKE) as? Secret.Present)?.value ?: return emptyList()
        return runCatching {
            val arr = JSONArray(raw)
            (0 until arr.length()).map { i ->
                arr.getJSONObject(i).let { PendingRevoke(it.getString("server"), it.getString("loginName"), it.getString("appPassword")) }
            }
        }.getOrDefault(emptyList())
    }

    fun addPendingRevoke(p: PendingRevoke) {
        if (read(KEY_REVOKE) == Secret.Unreadable) return
        writeRevokes(pendingRevokes().filter { it != p } + p)
    }

    fun dropPendingRevoke(p: PendingRevoke) {
        if (read(KEY_REVOKE) == Secret.Unreadable) return
        writeRevokes(pendingRevokes().filter { it != p })
    }

    @SuppressLint("ApplySharedPref")
    private fun writeRevokes(list: List<PendingRevoke>) {
        val edit = prefs.edit()
        if (list.isEmpty()) {
            edit.remove(KEY_REVOKE)
        } else {
            val arr = JSONArray()
            list.forEach { arr.put(JSONObject().put("server", it.server).put("loginName", it.loginName).put("appPassword", it.appPassword)) }
            val sealed = runCatching { encrypt(arr.toString()) }.getOrElse {
                Log.e(TAG, "revoke list not written", it)
                return
            }
            edit.putString(KEY_REVOKE, sealed)
        }
        edit.commit()
    }

    /** Data folder for a new account: `accounts/<id>/` next to the old `talk/`. */
    fun newDir(id: String): String = File(context.noBackupFilesDir, "accounts/$id").path

    private fun read(prefKey: String): Secret {
        val encoded = prefs.getString(prefKey, null) ?: return Secret.Missing
        return try {
            Secret.Present(decrypt(encoded))
        } catch (e: Exception) {
            Log.e(TAG, "secret not readable", e)
            Secret.Unreadable
        }
    }

    /** For encrypting: created on first use. */
    private fun key(): SecretKey {
        existingKey()?.let { return it }
        val gen = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
        gen.init(
            KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .build(),
        )
        return gen.generateKey()
    }

    private fun existingKey(): SecretKey? {
        val ks = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        return (ks.getEntry(ALIAS, null) as? KeyStore.SecretKeyEntry)?.secretKey
    }

    private fun encrypt(plain: String): String {
        val cipher = Cipher.getInstance(TRANSFORMATION).apply { init(Cipher.ENCRYPT_MODE, key()) }
        val data = cipher.iv + cipher.doFinal(plain.toByteArray())
        return Base64.encodeToString(data, Base64.NO_WRAP)
    }

    /** Never creates a key: a missing keystore key is a read error, not a new start. */
    private fun decrypt(encoded: String): String {
        val key = existingKey() ?: throw IllegalStateException("keystore key missing")
        val data = Base64.decode(encoded, Base64.NO_WRAP)
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(Cipher.DECRYPT_MODE, key, GCMParameterSpec(128, data, 0, IV_SIZE))
        return String(cipher.doFinal(data, IV_SIZE, data.size - IV_SIZE))
    }

    private fun AccountEntry.toJson() = JSONObject()
        .put("id", id).put("server", server).put("loginName", loginName)
        .put("userId", userId).put("name", name).put("key", key).put("dir", dir)

    private fun JSONObject.toEntry() = AccountEntry(
        id = getString("id"),
        server = getString("server"),
        loginName = getString("loginName"),
        userId = optString("userId"),
        name = optString("name"),
        key = optString("key"),
        dir = getString("dir"),
    )

    private fun List<AccountEntry>.toJson() = JSONArray().also { arr -> forEach { arr.put(it.toJson()) } }.toString()

    private companion object {
        const val TAG = "TalkAccounts"
        const val ALIAS = "talk-account"
        const val TRANSFORMATION = "AES/GCM/NoPadding"
        const val IV_SIZE = 12
        const val KEY_ACCOUNTS = "accounts"
        const val KEY_ACTIVE = "active"
        const val KEY_REVOKE = "revoke"
        /** The single account of earlier versions. */
        val LEGACY = listOf("server", "login", "secret", "push_key")

        fun secretKey(id: String) = "secret.$id"
        fun pushKeyKey(id: String) = "push_key.$id"
    }
}
