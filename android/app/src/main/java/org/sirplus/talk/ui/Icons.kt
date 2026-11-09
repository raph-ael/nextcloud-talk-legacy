package org.sirplus.talk.ui

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.addPathNodes
import androidx.compose.ui.unit.dp

/**
 * The few outlined Material icons the app needs (Material Icons, Apache-2.0), built from
 * their SVG path data instead of pulling in the multi-megabyte extended icon artifact.
 */
object TalkIcons {
    val Mood = icon(
        "Mood",
        "M15.5 9.5m-1.5 0a1.5 1.5 0 1 0 3 0a1.5 1.5 0 1 0 -3 0" +
            "M8.5 9.5m-1.5 0a1.5 1.5 0 1 0 3 0a1.5 1.5 0 1 0 -3 0" +
            "M11.99 2C6.47 2 2 6.48 2 12s4.47 10 9.99 10C17.52 22 22 17.52 22 12S17.52 2 11.99 2z" +
            "M12 20c-4.42 0-8-3.58-8-8s3.58-8 8-8 8 3.58 8 8-3.58 8-8 8z" +
            "m0-4c-1.48 0-2.75-0.81-3.45-2H6.88c0.8 2.05 2.79 3.5 5.12 3.5s4.32-1.45 5.12-3.5h-1.67c-0.7 1.19-1.97 2-3.45 2z",
    )
    val Keyboard = icon(
        "Keyboard",
        "M20 5H4c-1.1 0-1.99 0.9-1.99 2L2 17c0 1.1 0.9 2 2 2h16c1.1 0 2-0.9 2-2V7c0-1.1-0.9-2-2-2z" +
            "m0 12H4V7h16v10zm-9-9h2v2h-2zm0 3h2v2h-2zM8 8h2v2H8zm0 3h2v2H8zm-3 0h2v2H5zm0-3h2v2H5z" +
            "m3 6h8v2H8zm6-3h2v2h-2zm0-3h2v2h-2zm3 3h2v2h-2zm0-3h2v2h-2z",
    )
    val Camera = icon(
        "Camera",
        "M14.12 4l1.83 2H20v12H4V6h4.05l1.83-2h4.24M15 2H9L7.17 4H4c-1.1 0-2 0.9-2 2v12" +
            "c0 1.1 0.9 2 2 2h16c1.1 0 2-0.9 2-2V6c0-1.1-0.9-2-2-2h-3.17L15 2z" +
            "m-3 7c1.65 0 3 1.35 3 3s-1.35 3-3 3-3-1.35-3-3 1.35-3 3-3m0-2c-2.76 0-5 2.24-5 5s2.24 5 5 5 5-2.24 5-5-2.24-5-5-5z",
    )
    val Mic = icon(
        "Mic",
        "M12 14c1.66 0 2.99-1.34 2.99-3L15 5c0-1.66-1.34-3-3-3S9 3.34 9 5v6c0 1.66 1.34 3 3 3z" +
            "m-1.2-9.1c0-0.66 0.54-1.2 1.2-1.2 0.66 0 1.2 0.54 1.2 1.2l-0.01 6.2c0 0.66-0.53 1.2-1.19 1.2" +
            "-0.66 0-1.2-0.54-1.2-1.2V4.9zm6.5 6.1c0 3-2.54 5.1-5.3 5.1S6.7 14 6.7 11H5c0 3.41 2.72 6.23 6 6.72V21h2" +
            "v-3.28c3.28-0.48 6-3.3 6-6.72h-1.7z",
    )
    val Plus = icon("Plus", "M19 13h-6v6h-2v-6H5v-2h6V5h2v6h6v2z")
    val Play = icon("Play", "M8 5v14l11-7z")
    val Pause = icon("Pause", "M6 19h4V5H6v14zm8-14v14h4V5h-4z")
    val Lock = icon(
        "Lock",
        "M18 8h-1V6c0-2.76-2.24-5-5-5S7 3.24 7 6v2H6c-1.1 0-2 0.9-2 2v10c0 1.1 0.9 2 2 2h12c1.1 0 2-0.9 2-2V10" +
            "c0-1.1-0.9-2-2-2zM9 6c0-1.66 1.34-3 3-3s3 1.34 3 3v2H9V6zm9 14H6V10h12v10z" +
            "m-6-3c1.1 0 2-0.9 2-2s-0.9-2-2-2-2 0.9-2 2 0.9 2 2 2z",
    )
    val ArrowUp = icon("ArrowUp", "M7.41 15.41L12 10.83l4.59 4.58L18 14l-6-6-6 6z")
    val Delete = icon(
        "Delete",
        "M16 9v10H8V9h8m-1.5-6h-5l-1 1H5v2h14V4h-3.5l-1-1zM18 7H6v12c0 1.1 0.9 2 2 2h8c1.1 0 2-0.9 2-2V7z",
    )
    val Send = icon("Send", "M2.01 21L23 12 2.01 3 2 10l15 2-15 2z")
    val Close = icon(
        "Close",
        "M19 6.41L17.59 5 12 10.59 6.41 5 5 6.41 10.59 12 5 17.59 6.41 19 12 13.41 17.59 19 19 17.59 13.41 12z",
    )
    val Backspace = icon(
        "Backspace",
        "M22 3H7c-0.69 0-1.23 0.35-1.59 0.88L0 12l5.41 8.11c0.36 0.53 0.9 0.89 1.59 0.89h15c1.1 0 2-0.9 2-2V5" +
            "c0-1.1-0.9-2-2-2zm0 16H7.07L2.4 12l4.66-7H22v14zm-11.59-2L14 13.41 17.59 17 19 15.59 15.41 12 19 8.41" +
            " 17.59 7 14 10.59 10.41 7 9 8.41 12.59 12 9 15.59z",
    )
    val Gallery = icon(
        "Gallery",
        "M20 4v12H8V4h12m0-2H8c-1.1 0-2 0.9-2 2v12c0 1.1 0.9 2 2 2h12c1.1 0 2-0.9 2-2V4c0-1.1-0.9-2-2-2z" +
            "m-8.5 9.67l1.69 2.26 2.48-3.1L19 15H9zM2 6v14c0 1.1 0.9 2 2 2h14v-2H4V6H2z",
    )
    val File = icon(
        "File",
        "M14 2H6c-1.1 0-1.99 0.9-1.99 2L4 20c0 1.1 0.89 2 1.99 2H18c1.1 0 2-0.9 2-2V8l-6-6zM6 20V4h7v5h5v11H6z",
    )
    val Poll = icon(
        "Poll",
        "M19 3H5c-1.1 0-2 0.9-2 2v14c0 1.1 0.9 2 2 2h14c1.1 0 2-0.9 2-2V5c0-1.1-0.9-2-2-2z" +
            "m0 16H5V5h14v14zM7 10h2v7H7zm4-3h2v10h-2zm4 6h2v4h-2z",
    )
    val Contact = icon(
        "Contact",
        "M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2z" +
            "M7.07 18.28c0.43-0.9 3.05-1.78 4.93-1.78s4.51 0.88 4.93 1.78C15.57 19.36 13.86 20 12 20s-3.57-0.64-4.93-1.72z" +
            "m11.29-1.45c-1.43-1.74-4.9-2.33-6.36-2.33s-4.93 0.59-6.36 2.33C4.62 15.49 4 13.82 4 12c0-4.41 3.59-8 8-8" +
            "s8 3.59 8 8c0 1.82-0.62 3.49-1.64 4.83zM12 6c-1.94 0-3.5 1.56-3.5 3.5S10.06 13 12 13s3.5-1.56 3.5-3.5" +
            "S13.94 6 12 6zm0 5c-0.83 0-1.5-0.67-1.5-1.5S11.17 8 12 8s1.5 0.67 1.5 1.5S12.83 11 12 11z",
    )
    val Location = icon(
        "Location",
        "M12 2C8.13 2 5 5.13 5 9c0 5.25 7 13 7 13s7-7.75 7-13c0-3.87-3.13-7-7-7z" +
            "M7 9c0-2.76 2.24-5 5-5s5 2.24 5 5c0 2.88-2.88 7.19-5 9.88C9.92 16.21 7 11.85 7 9z" +
            "m5-2.5c-1.38 0-2.5 1.12-2.5 2.5s1.12 2.5 2.5 2.5 2.5-1.12 2.5-2.5-1.12-2.5-2.5-2.5z",
    )
    val FlashOn = icon("FlashOn", "M7 2v11h3v9l7-12h-4l4-8z")
    val FlashOff = icon(
        "FlashOff",
        "M3.27 3L2 4.27l5 5V13h3v9l3.58-6.14L17.73 20 19 18.73 3.27 3zM17 10h-4l4-8H7v2.18l8.46 8.46L17 10z",
    )
    val FlashAuto = icon(
        "FlashAuto",
        "M3 2v12h3v9l7-12H9l4-9H3zm16 0h-2l-3.2 9h1.9l0.7-2h3.2l0.7 2h1.9L19 2zm-2.15 5.65L18 4l1.15 3.65h-2.3z",
    )
    val SwitchCamera = icon(
        "SwitchCamera",
        "M16 7h-1l-1-1h-4L9 7H8c-1.1 0-2 0.9-2 2v6c0 1.1 0.9 2 2 2h8c1.1 0 2-0.9 2-2V9c0-1.1-0.9-2-2-2z" +
            "m-4 7c-1.1 0-2-0.9-2-2s0.9-2 2-2 2 0.9 2 2-0.9 2-2 2z" +
            "M8.57 0.51l4.48 4.48V2.04c4.72 0.47 8.48 4.23 8.95 8.95h2C23.34 3.02 15.49-1.59 8.57 0.51z" +
            "m2.38 21.45c-4.72-0.47-8.48-4.23-8.95-8.95H0c0.66 7.97 8.51 12.58 15.43 10.48l-4.48-4.48v2.95z",
    )

    private fun icon(name: String, path: String): ImageVector =
        ImageVector.Builder(name, 24.dp, 24.dp, 24f, 24f)
            .addPath(addPathNodes(path), fill = SolidColor(Color.Black))
            .build()
}
