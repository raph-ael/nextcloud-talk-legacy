package org.wolkenfunk.app.media

import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Canvas
import android.util.LruCache
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.async
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.sync.Semaphore
import kotlinx.coroutines.sync.withPermit
import kotlinx.coroutines.withContext
import org.wolkenfunk.app.BuildConfig
import java.io.File
import java.net.HttpURLConnection
import java.net.URL
import kotlin.math.PI
import kotlin.math.cos
import kotlin.math.floor
import kotlin.math.ln
import kotlin.math.log2
import kotlin.math.roundToInt
import kotlin.math.tan

/**
 * Map pictures for places (docs/attachments.md „Kartenquelle“): tiles of tile.openstreetmap.org
 * put together into one image with the place in its centre. The rules of the tile server hold:
 * at most six tiles per image, two requests at a time, our own User-Agent, and every tile stays
 * in the app's cache for at least seven days (20 MB at most). Nothing is loaded ahead.
 */
object MapTiles {
    /** Size of the picture in tile pixels, 240 : 150 like the bubble: it never spans more than 3 × 2 tiles. */
    const val WIDTH = 410
    const val HEIGHT = 256

    private const val TILE = 256
    private const val MAX_ZOOM = 19
    private const val KEEP_MS = 7L * 24 * 3600 * 1000
    private const val CACHE_BYTES = 20L * 1024 * 1024
    /** Metres per tile pixel at the equator, zoom 0. */
    private const val EQUATOR_M_PER_PX = 156543.03392

    private val requests = Semaphore(2)
    private val pictures = LruCache<String, Bitmap>(16)

    /** Which part of the world map a picture shows: the zoom and its top left corner in world pixels. */
    private data class View(val zoom: Int, val left: Int, val top: Int) {
        val key get() = "$zoom/$left/$top"
    }

    /** The view that is about 1 km wide around the place (0.7 to 1.4 km, by the zoom steps). */
    private fun viewOf(latitude: Double, longitude: Double): View {
        val lat = latitude.coerceIn(-85.0511, 85.0511)
        val phi = lat * PI / 180
        val zoom = log2(EQUATOR_M_PER_PX * cos(phi) * WIDTH / 1000.0).roundToInt().coerceIn(1, MAX_ZOOM)
        val world = TILE.toDouble() * (1 shl zoom)
        val x = (longitude + 180.0) / 360.0 * world
        val y = (1 - ln(tan(phi) + 1 / cos(phi)) / PI) / 2 * world
        return View(zoom, floor(x - WIDTH / 2.0).toInt(), floor(y - HEIGHT / 2.0).toInt())
    }

    /** The picture when it was put together before in this run (for the first frame). */
    fun cached(latitude: Double, longitude: Double): Bitmap? = pictures.get(viewOf(latitude, longitude).key)

    /** The picture around the place, or `null` without network or on any error (nothing is remembered then). */
    suspend fun picture(context: Context, latitude: Double, longitude: Double): Bitmap? {
        val view = viewOf(latitude, longitude)
        pictures.get(view.key)?.let { return it }
        val dir = File(context.cacheDir, "tiles")
        val count = 1 shl view.zoom
        val columns = Math.floorDiv(view.left, TILE)..Math.floorDiv(view.left + WIDTH - 1, TILE)
        val rows = Math.floorDiv(view.top, TILE)..Math.floorDiv(view.top + HEIGHT - 1, TILE)
        val wanted = rows.filter { it in 0 until count }.flatMap { ty -> columns.map { tx -> tx to ty } }
        val tiles = coroutineScope {
            wanted.map { (tx, ty) -> async { tile(dir, view.zoom, Math.floorMod(tx, count), ty) } }.awaitAll()
        }
        if (tiles.any { it == null }) return null
        val picture = withContext(Dispatchers.Default) {
            val out = Bitmap.createBitmap(WIDTH, HEIGHT, Bitmap.Config.ARGB_8888)
            val canvas = Canvas(out)
            canvas.drawColor(0xFFE8E8E8.toInt())
            wanted.forEachIndexed { i, (tx, ty) ->
                val bmp = tiles[i] ?: return@forEachIndexed
                canvas.drawBitmap(bmp, (tx * TILE - view.left).toFloat(), (ty * TILE - view.top).toFloat(), null)
                bmp.recycle()
            }
            out
        }
        pictures.put(view.key, picture)
        return picture
    }

    /** One tile: from the cache while it is younger than seven days, else from the server (the old one if that fails). */
    private suspend fun tile(dir: File, zoom: Int, x: Int, y: Int): Bitmap? = withContext(Dispatchers.IO) {
        val file = File(dir, "$zoom-$x-$y.png")
        val known = file.takeIf { it.isFile && it.length() > 0 }
        if (known != null && System.currentTimeMillis() - known.lastModified() < KEEP_MS) {
            BitmapFactory.decodeFile(known.path)?.let { return@withContext it }
        }
        val fresh = requests.withPermit { runCatching { download(zoom, x, y) }.getOrNull() }
        val decoded = fresh?.let { BitmapFactory.decodeByteArray(it, 0, it.size) }
        if (fresh != null && decoded != null) {
            runCatching {
                dir.mkdirs()
                val tmp = File.createTempFile("tile", null, dir)
                tmp.writeBytes(fresh)
                if (!tmp.renameTo(file)) tmp.delete()
                trim(dir)
            }
            return@withContext decoded
        }
        known?.let { BitmapFactory.decodeFile(it.path) }
    }

    private fun download(zoom: Int, x: Int, y: Int): ByteArray? {
        val conn = URL("https://tile.openstreetmap.org/$zoom/$x/$y.png").openConnection() as HttpURLConnection
        try {
            conn.connectTimeout = 10_000
            conn.readTimeout = 15_000
            conn.setRequestProperty("User-Agent", "Wolkenfunk/${BuildConfig.VERSION_NAME}")
            if (conn.responseCode != 200) return null
            return conn.inputStream.use { it.readBytes() }
        } finally {
            conn.disconnect()
        }
    }

    /** Keeps the tile cache below 20 MB: the oldest tiles go first. */
    @Synchronized
    private fun trim(dir: File) {
        val files = dir.listFiles()?.sortedBy { it.lastModified() } ?: return
        var total = files.sumOf { it.length() }
        for (f in files) {
            if (total <= CACHE_BYTES) break
            total -= f.length()
            f.delete()
        }
    }
}
