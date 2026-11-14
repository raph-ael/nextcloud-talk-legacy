package org.wolkenfunk.app.media

import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Matrix
import android.media.ExifInterface
import android.net.Uri
import java.io.File
import java.time.LocalDateTime
import java.time.format.DateTimeFormatter
import kotlin.math.max

/** An image ready for upload: re-encoded JPEG in the app's private staging folder. */
data class PreparedImage(val file: File, val name: String, val width: Int, val height: Int)

private const val MAX_EDGE = 2048
private const val JPEG_QUALITY = 85
private val nameFmt = DateTimeFormatter.ofPattern("yyyyMMdd_HHmmss")

/**
 * Downscales to at most [MAX_EDGE] px on the long side, applies the EXIF rotation and
 * re-encodes as JPEG. Re-encoding drops all metadata (GPS position, camera serial …),
 * like Signal does. Blocking: call off the main thread.
 */
fun prepareImage(context: Context, source: Uri): PreparedImage {
    val resolver = context.contentResolver
    val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
    resolver.openInputStream(source).use { BitmapFactory.decodeStream(it, null, bounds) }
    require(bounds.outWidth > 0) { "not an image: $source" }

    var sample = 1
    while (max(bounds.outWidth, bounds.outHeight) / (sample * 2) >= MAX_EDGE) sample *= 2
    val decoded = resolver.openInputStream(source).use {
        BitmapFactory.decodeStream(it, null, BitmapFactory.Options().apply { inSampleSize = sample })
    } ?: error("cannot decode $source")

    val rotation = runCatching {
        resolver.openInputStream(source).use { ExifInterface(it!!).rotationDegrees() }
    }.getOrDefault(0)
    val scale = minOf(1f, MAX_EDGE.toFloat() / max(decoded.width, decoded.height))
    val bitmap = if (rotation != 0 || scale < 1f) {
        val m = Matrix().apply {
            postScale(scale, scale)
            postRotate(rotation.toFloat())
        }
        Bitmap.createBitmap(decoded, 0, 0, decoded.width, decoded.height, m, true).also {
            if (it !== decoded) decoded.recycle()
        }
    } else {
        decoded
    }

    // Not the cache: the system may clear it before the outbox has taken the file over.
    val dir = File(context.noBackupFilesDir, "talk/staging").apply { mkdirs() }
    val name = "IMG_${LocalDateTime.now().format(nameFmt)}.jpg"
    val file = File.createTempFile("img", ".jpg", dir)
    file.outputStream().use { bitmap.compress(Bitmap.CompressFormat.JPEG, JPEG_QUALITY, it) }
    val result = PreparedImage(file, name, bitmap.width, bitmap.height)
    bitmap.recycle()
    return result
}

/** Decodes [file] so that it fits roughly into [maxEdge] px (for previews). */
fun decodeSampled(file: File, maxEdge: Int): Bitmap? {
    val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
    BitmapFactory.decodeFile(file.path, bounds)
    if (bounds.outWidth <= 0) return null
    var sample = 1
    while (max(bounds.outWidth, bounds.outHeight) / (sample * 2) >= maxEdge) sample *= 2
    return BitmapFactory.decodeFile(file.path, BitmapFactory.Options().apply { inSampleSize = sample })
}

/** Decodes a content [uri] so that it fits roughly into [maxEdge] px, honouring EXIF rotation. */
fun decodeSampled(context: Context, uri: Uri, maxEdge: Int): Bitmap? {
    val resolver = context.contentResolver
    val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
    resolver.openInputStream(uri).use { BitmapFactory.decodeStream(it, null, bounds) }
    if (bounds.outWidth <= 0) return null
    var sample = 1
    while (max(bounds.outWidth, bounds.outHeight) / (sample * 2) >= maxEdge) sample *= 2
    val bmp = resolver.openInputStream(uri).use {
        BitmapFactory.decodeStream(it, null, BitmapFactory.Options().apply { inSampleSize = sample })
    } ?: return null
    val rotation = runCatching { resolver.openInputStream(uri).use { ExifInterface(it!!).rotationDegrees() } }
        .getOrDefault(0)
    if (rotation == 0) return bmp
    val m = Matrix().apply { postRotate(rotation.toFloat()) }
    return Bitmap.createBitmap(bmp, 0, 0, bmp.width, bmp.height, m, true)
}

private fun ExifInterface.rotationDegrees(): Int = when (
    getAttributeInt(ExifInterface.TAG_ORIENTATION, ExifInterface.ORIENTATION_NORMAL)
) {
    ExifInterface.ORIENTATION_ROTATE_90, ExifInterface.ORIENTATION_TRANSPOSE -> 90
    ExifInterface.ORIENTATION_ROTATE_180 -> 180
    ExifInterface.ORIENTATION_ROTATE_270, ExifInterface.ORIENTATION_TRANSVERSE -> 270
    else -> 0
}

/** Aspect ratio (width / height) helper that tolerates unknown sizes. */
fun aspectOf(width: Int, height: Int): Float =
    if (width > 0 && height > 0) (width.toFloat() / height).coerceIn(0.4f, 2.5f) else 1f
