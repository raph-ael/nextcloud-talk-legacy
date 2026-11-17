package org.wolkenfunk.app.ui

import android.Manifest
import android.content.ActivityNotFoundException
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.PackageManager
import android.graphics.Bitmap
import android.location.Address
import android.location.Geocoder
import android.location.Location
import android.location.LocationManager
import android.net.Uri
import android.os.Build
import android.os.Looper
import android.os.SystemClock
import android.provider.Settings
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.layout.onPlaced
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.content.ContextCompat
import androidx.core.location.LocationListenerCompat
import androidx.core.location.LocationManagerCompat
import androidx.core.location.LocationRequestCompat
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LifecycleEventEffect
import androidx.lifecycle.compose.LifecycleStartEffect
import kotlinx.coroutines.delay
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.withTimeoutOrNull
import org.wolkenfunk.app.media.MapTiles
import java.util.Locale
import kotlin.concurrent.thread
import kotlin.coroutines.resume

/** Size of the map in a bubble (docs/attachments.md „Blase“). */
val LocationMapWidth = 240.dp
val LocationMapHeight = 150.dp

private const val SEARCH_MS = 30_000L
private const val ADDRESS_MS = 5_000L

/**
 * Map picture around a place with the pin on it. The tiles load only once the picture is on
 * screen; without network or on an error the area stays in `chip` with the pin, and the next
 * try comes when it is shown again (docs/attachments.md „Kartenquelle“).
 */
@Composable
fun LocationMap(latitude: Double, longitude: Double, modifier: Modifier = Modifier, pin: Dp = 36.dp) {
    val context = LocalContext.current.applicationContext
    val colors = LocalTalkColors.current
    // Composed ahead of the visible area by the list, placed only when it scrolls in.
    var placed by remember { mutableStateOf(false) }
    // The picture with the place it was made for: it never stays under a pin that is somewhere else.
    var shown by remember { mutableStateOf(MapTiles.cached(latitude, longitude)?.let { Shown(latitude, longitude, it) }) }
    LaunchedEffect(latitude, longitude, placed) {
        if (shown?.at(latitude, longitude) == true) return@LaunchedEffect
        MapTiles.cached(latitude, longitude)?.let {
            shown = Shown(latitude, longitude, it)
            return@LaunchedEffect
        }
        // While the new picture loads the old one stays if the place moved by a few metres only.
        if (shown?.near(latitude, longitude) != true) shown = null
        if (placed) shown = MapTiles.picture(context, latitude, longitude)?.let { Shown(latitude, longitude, it) }
    }
    val picture = shown?.picture
    Box(modifier.onPlaced { placed = true }.background(colors.chip)) {
        picture?.let { bmp ->
            Image(remember(bmp) { bmp.asImageBitmap() }, null, contentScale = ContentScale.Crop, modifier = Modifier.fillMaxSize())
            Text(
                "© OpenStreetMap",
                color = Color(0xFF333333),
                fontSize = 9.sp,
                lineHeight = 11.sp,
                modifier = Modifier
                    .align(Alignment.BottomEnd)
                    .clearAndSetSemantics {}
                    .background(Color.White.copy(alpha = 0.7f), RoundedCornerShape(topStart = 4.dp))
                    .padding(horizontal = 4.dp, vertical = 1.dp),
            )
        }
        // The tip of the pin (22 of 24 down) sits on the place in the centre.
        Icon(
            TalkIcons.Pin,
            null,
            tint = colors.accent,
            modifier = Modifier.align(Alignment.Center).offset(y = -pin * (22f / 24f - 0.5f)).size(pin),
        )
    }
}

private class Shown(val latitude: Double, val longitude: Double, val picture: Bitmap) {
    fun at(lat: Double, lon: Double) = lat == latitude && lon == longitude

    fun near(lat: Double, lon: Double): Boolean {
        val d = FloatArray(1)
        Location.distanceBetween(latitude, longitude, lat, lon, d)
        return d[0] < 100f
    }
}

/** Opens the place in the map app with pin and name; without one, on openstreetmap.org in the browser. */
fun openInMaps(context: Context, latitude: Double, longitude: Double, name: String) {
    fun view(uri: String) =
        context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(uri)).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
    try {
        view(geoUri(latitude, longitude, name))
    } catch (e: ActivityNotFoundException) {
        runCatching { view(osmLink(latitude, longitude)) }
    }
}

private fun hasLocationAccess(context: Context): Boolean =
    listOf(Manifest.permission.ACCESS_FINE_LOCATION, Manifest.permission.ACCESS_COARSE_LOCATION)
        .any { ContextCompat.checkSelfPermission(context, it) == PackageManager.PERMISSION_GRANTED }

/** From API 31 on the system's fused provider, else network and GPS. */
private fun providers(manager: LocationManager): List<String> {
    if (Build.VERSION.SDK_INT >= 31 && manager.hasProvider(LocationManager.FUSED_PROVIDER)) {
        return listOf(LocationManager.FUSED_PROVIDER)
    }
    val all = manager.allProviders
    return listOf(LocationManager.NETWORK_PROVIDER, LocationManager.GPS_PROVIDER).filter { it in all }
}

private fun Location.ageMs(): Long = (SystemClock.elapsedRealtimeNanos() - elapsedRealtimeNanos) / 1_000_000

/** The more exact position wins; a much newer one replaces an old one in any case. */
private fun better(old: Location?, new: Location): Location = when {
    old == null -> new
    !old.hasAccuracy() || (new.hasAccuracy() && new.accuracy <= old.accuracy) -> new
    new.elapsedRealtimeNanos - old.elapsedRealtimeNanos > 10_000_000_000L -> new
    else -> old
}

/** „{Straße} {Hausnummer}, {Ort}“, without a street „{Ort}“; `null` when the address says neither. */
private fun Address.label(): String? {
    val street = listOfNotNull(thoroughfare, subThoroughfare).joinToString(" ").trim()
    val town = (locality ?: subAdminArea).orEmpty().trim()
    return listOf(street, town).filter { it.isNotEmpty() }.joinToString(", ").ifEmpty { null }
}

/** The address of the system for a position; `null` without the service, without network or after 5 s. */
private suspend fun addressOf(context: Context, latitude: Double, longitude: Double): String? {
    if (!Geocoder.isPresent()) return null
    val geocoder = Geocoder(context, Locale.getDefault())
    return withTimeoutOrNull(ADDRESS_MS) {
        suspendCancellableCoroutine<String?> { cont ->
            if (Build.VERSION.SDK_INT >= 33) {
                val listener = object : Geocoder.GeocodeListener {
                    override fun onGeocode(addresses: MutableList<Address>) {
                        if (cont.isActive) cont.resume(addresses.firstOrNull()?.label())
                    }

                    override fun onError(errorMessage: String?) {
                        if (cont.isActive) cont.resume(null)
                    }
                }
                runCatching { geocoder.getFromLocation(latitude, longitude, 1, listener) }
                    .onFailure { if (cont.isActive) cont.resume(null) }
            } else {
                // Blocks until the service answers: off the coroutine, so the 5 s hold.
                thread(name = "geocoder") {
                    @Suppress("DEPRECATION")
                    val found = runCatching { geocoder.getFromLocation(latitude, longitude, 1)?.firstOrNull()?.label() }.getOrNull()
                    if (cont.isActive) cont.resume(found)
                }
            }
        }
    }
}

/**
 * „Standort senden“ (docs/attachments.md): asks for the permission, searches the own position
 * while the sheet is open and the app in front, and shows it on the map with its address.
 * Closing the sheet ends the search.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun LocationSheet(onSend: (latitude: Double, longitude: Double, name: String) -> Unit, onDismiss: () -> Unit) {
    val context = LocalContext.current
    val colors = LocalTalkColors.current
    val manager = remember { context.getSystemService(Context.LOCATION_SERVICE) as LocationManager }

    var granted by remember { mutableStateOf(hasLocationAccess(context)) }
    // The system's question is on screen (or still to come): no „not allowed“ meanwhile.
    var asking by remember { mutableStateOf(!granted) }
    var enabled by remember { mutableStateOf(LocationManagerCompat.isLocationEnabled(manager)) }
    var attempt by remember { mutableIntStateOf(0) }
    var notFound by remember { mutableStateOf(false) }
    var fix by remember { mutableStateOf<Location?>(null) }
    // Where the address is asked for, and the answer (`null`: none, „Standort“).
    var addressAt by remember { mutableStateOf<Location?>(null) }
    var address by remember { mutableStateOf<String?>(null) }

    val request = rememberLauncherForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) {
        granted = hasLocationAccess(context)
        asking = false
    }
    LaunchedEffect(Unit) {
        if (!granted) {
            request.launch(arrayOf(Manifest.permission.ACCESS_FINE_LOCATION, Manifest.permission.ACCESS_COARSE_LOCATION))
        }
    }
    // Back from the settings: permission and positioning may have changed.
    LifecycleEventEffect(Lifecycle.Event.ON_RESUME) {
        granted = hasLocationAccess(context)
        enabled = LocationManagerCompat.isLocationEnabled(manager)
    }
    // Positioning switched in the quick settings, without leaving the app.
    DisposableEffect(Unit) {
        val receiver = object : BroadcastReceiver() {
            override fun onReceive(c: Context, intent: Intent) {
                enabled = LocationManagerCompat.isLocationEnabled(manager)
            }
        }
        ContextCompat.registerReceiver(
            context, receiver, IntentFilter(LocationManager.MODE_CHANGED_ACTION), ContextCompat.RECEIVER_NOT_EXPORTED,
        )
        onDispose { context.unregisterReceiver(receiver) }
    }

    val searching = granted && enabled && !notFound
    // Positions come only between start and stop of the activity: never in the background.
    LifecycleStartEffect(searching, attempt) {
        val listener = LocationListenerCompat { loc -> fix = better(fix, loc) }
        // High accuracy: without it the fused provider leaves GPS off and answers from WLAN and cells only.
        val wanted = LocationRequestCompat.Builder(1000L).setQuality(LocationRequestCompat.QUALITY_HIGH_ACCURACY).build()
        if (searching) {
            for (provider in providers(manager)) {
                runCatching {
                    @Suppress("MissingPermission")
                    manager.getLastKnownLocation(provider)?.takeIf { it.ageMs() < 120_000 }?.let { fix = better(fix, it) }
                    @Suppress("MissingPermission")
                    LocationManagerCompat.requestLocationUpdates(manager, provider, wanted, listener, Looper.getMainLooper())
                }
            }
        }
        onStopOrDispose {
            @Suppress("MissingPermission")
            runCatching { LocationManagerCompat.removeUpdates(manager, listener) }
        }
    }
    LaunchedEffect(searching, attempt) {
        if (searching && fix == null) {
            delay(SEARCH_MS)
            if (fix == null) notFound = true
        }
    }
    // The address follows the position once it moved by more than 50 m; sending never waits for it.
    val at = fix
    if (at != null && addressAt.let { it == null || it.distanceTo(at) >= 50f }) addressAt = at
    LaunchedEffect(addressAt) {
        val asked = addressAt ?: return@LaunchedEffect
        // The old address stays until the answer is there.
        address = addressOf(context.applicationContext, asked.latitude, asked.longitude)
    }

    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)) {
        Column(Modifier.fillMaxWidth().padding(start = 20.dp, end = 20.dp, bottom = 20.dp)) {
            Text(
                "Standort senden",
                style = ListType.section,
                modifier = Modifier.padding(bottom = 14.dp).semantics { heading() },
            )
            val area = Modifier.fillMaxWidth().aspectRatio(LocationMapWidth / LocationMapHeight).clip(RoundedCornerShape(14.dp))
            val name = locationName(address.orEmpty())
            if (at != null && searching) {
                LocationMap(at.latitude, at.longitude, area, pin = 44.dp)
                Text(
                    name,
                    fontSize = 16.sp,
                    fontWeight = FontWeight.Medium,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.padding(top = 12.dp),
                )
                Text(
                    if (at.hasAccuracy()) accuracyLabel(at.accuracy) else "",
                    color = colors.muted,
                    fontSize = 14.sp,
                    modifier = Modifier.padding(top = 2.dp),
                )
            } else {
                Box(area.background(colors.chip), contentAlignment = Alignment.Center) {
                    Column(Modifier.padding(20.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                        when {
                            asking -> CircularProgressIndicator(Modifier.size(28.dp), strokeWidth = 3.dp)
                            !granted -> Notice("Wolkenfunk darf den Standort nicht verwenden", "Einstellungen öffnen") {
                                runCatching {
                                    context.startActivity(
                                        Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.fromParts("package", context.packageName, null)),
                                    )
                                }
                            }
                            !enabled -> Notice("Die Ortung ist ausgeschaltet", "Einstellungen öffnen") {
                                runCatching { context.startActivity(Intent(Settings.ACTION_LOCATION_SOURCE_SETTINGS)) }
                            }
                            notFound -> Notice("Standort nicht gefunden", "Erneut versuchen") {
                                notFound = false
                                attempt++
                            }
                            else -> {
                                CircularProgressIndicator(Modifier.size(28.dp), strokeWidth = 3.dp)
                                Text("Standort wird bestimmt …", color = colors.muted, fontSize = 15.sp, modifier = Modifier.padding(top = 12.dp))
                            }
                        }
                    }
                }
                // Keeps the sheet as tall as with a position.
                Text("", fontSize = 16.sp, modifier = Modifier.padding(top = 12.dp))
                Text("", fontSize = 14.sp, modifier = Modifier.padding(top = 2.dp))
            }
            Button(
                onClick = { at?.let { onSend(it.latitude, it.longitude, name) } },
                enabled = at != null && searching,
                modifier = Modifier.fillMaxWidth().padding(top = 16.dp),
            ) {
                Text("Diesen Standort senden")
            }
        }
    }
}

@Composable
private fun Notice(text: String, action: String, onAction: () -> Unit) {
    Text(text, color = MaterialTheme.colorScheme.onSurface, fontSize = 15.sp, textAlign = TextAlign.Center)
    TextButton(onClick = onAction) { Text(action) }
}
