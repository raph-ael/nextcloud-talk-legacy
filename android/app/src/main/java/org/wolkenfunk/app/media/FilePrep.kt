package org.wolkenfunk.app.media

import android.content.Context
import android.graphics.BitmapFactory
import android.media.ExifInterface
import android.net.Uri
import android.provider.OpenableColumns
import java.io.File

/** A file chosen in the system's picker, as the send screen lists it. [size] is −1 when unknown. */
data class PickedFile(val uri: Uri, val name: String, val size: Long)

/** A file ready for the outbox: an unchanged copy in the staging folder. */
data class PreparedFile(val file: File, val name: String, val mime: String, val width: Int, val height: Int)

/** Name and size as the system reports them. Blocking: call off the main thread. */
fun describeFile(context: Context, uri: Uri): PickedFile {
    var name = ""
    var size = -1L
    runCatching {
        context.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE), null, null, null)?.use { c ->
            if (c.moveToFirst()) {
                val n = c.getColumnIndex(OpenableColumns.DISPLAY_NAME)
                val s = c.getColumnIndex(OpenableColumns.SIZE)
                if (n >= 0 && !c.isNull(n)) name = c.getString(n).orEmpty()
                if (s >= 0 && !c.isNull(s)) size = c.getLong(s)
            }
        }
    }
    if (name.isBlank()) name = uri.lastPathSegment?.substringAfterLast('/').orEmpty()
    return PickedFile(uri, name.ifBlank { "Datei" }, size)
}

/**
 * Copies [picked] byte for byte into the staging folder (docs/attachments.md „Inhalt“): nothing
 * is re-encoded, an image keeps its size and metadata. The MIME type is the system's, without
 * one `application/octet-stream`; images get their size from the file header. Throws when the
 * file cannot be read. Blocking: call off the main thread.
 */
fun prepareFile(context: Context, picked: PickedFile): PreparedFile {
    val resolver = context.contentResolver
    val dir = stagingDir(context).apply { mkdirs() }
    val file = File.createTempFile("file", null, dir)
    try {
        val input = resolver.openInputStream(picked.uri) ?: error("cannot open ${picked.uri}")
        input.use { src -> file.outputStream().use { src.copyTo(it, 256 * 1024) } }
        val mime = runCatching { resolver.getType(picked.uri) }.getOrNull()?.takeIf { it.isNotBlank() }
            ?: "application/octet-stream"
        var width = 0
        var height = 0
        if (mime.startsWith("image/")) {
            val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
            BitmapFactory.decodeFile(file.path, bounds)
            if (bounds.outWidth > 0 && bounds.outHeight > 0) {
                // As it is shown: a photo stored on its side counts upright.
                val turned = runCatching { ExifInterface(file.path).rotationDegrees() % 180 != 0 }.getOrDefault(false)
                width = if (turned) bounds.outHeight else bounds.outWidth
                height = if (turned) bounds.outWidth else bounds.outHeight
            }
        }
        return PreparedFile(file, picked.name, mime, width, height)
    } catch (e: Exception) {
        file.delete()
        throw e
    }
}
