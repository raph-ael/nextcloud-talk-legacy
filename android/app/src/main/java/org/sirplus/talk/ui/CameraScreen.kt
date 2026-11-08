package org.sirplus.talk.ui

import android.Manifest
import android.content.pm.PackageManager
import android.net.Uri
import android.widget.Toast
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.camera.core.CameraSelector
import androidx.camera.core.ImageCapture
import androidx.camera.core.ImageCaptureException
import androidx.camera.view.CameraController
import androidx.camera.view.LifecycleCameraController
import androidx.camera.view.PreviewView
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.core.content.ContextCompat
import androidx.lifecycle.compose.LocalLifecycleOwner
import kotlinx.coroutines.launch
import java.io.File

/** Returns a function that opens the camera, asking for the permission first if needed. */
@Composable
fun rememberCameraOpener(onGranted: () -> Unit): () -> Unit {
    val context = LocalContext.current
    val launcher = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { ok ->
        if (ok) onGranted() else Toast.makeText(context, "Ohne Kamera-Berechtigung geht das leider nicht", Toast.LENGTH_SHORT).show()
    }
    return {
        if (ContextCompat.checkSelfPermission(context, Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED) {
            onGranted()
        } else {
            launcher.launch(Manifest.permission.CAMERA)
        }
    }
}

/**
 * In-app camera like Signal's: opens instantly, tap to focus, pinch to zoom (both built into
 * [LifecycleCameraController]), flash off/auto/on, front/back. Photos land in the app cache.
 */
@Composable
fun CameraScreen(onClose: () -> Unit, onCaptured: (Uri) -> Unit, onGallery: () -> Unit) {
    val context = LocalContext.current
    val lifecycleOwner = LocalLifecycleOwner.current
    val scope = rememberCoroutineScope()
    val controller = remember {
        LifecycleCameraController(context).apply {
            setEnabledUseCases(CameraController.IMAGE_CAPTURE)
            imageCaptureMode = ImageCapture.CAPTURE_MODE_MINIMIZE_LATENCY
        }
    }
    DisposableEffect(lifecycleOwner) {
        controller.bindToLifecycle(lifecycleOwner)
        onDispose { controller.unbind() }
    }
    var flash by remember { mutableIntStateOf(ImageCapture.FLASH_MODE_OFF) }
    var front by remember { mutableStateOf(false) }
    var busy by remember { mutableStateOf(false) }
    val blink = remember { Animatable(0f) }
    BackHandler(onBack = onClose)

    Box(Modifier.fillMaxSize().background(Color.Black)) {
        AndroidView(
            factory = { ctx ->
                PreviewView(ctx).apply {
                    scaleType = PreviewView.ScaleType.FILL_CENTER
                    this.controller = controller
                }
            },
            modifier = Modifier.fillMaxSize(),
        )
        // Shutter feedback.
        Box(Modifier.fillMaxSize().graphicsLayer { alpha = blink.value }.background(Color.Black))

        Row(
            Modifier.fillMaxWidth().statusBarsPadding().padding(8.dp),
            horizontalArrangement = Arrangement.SpaceBetween,
        ) {
            IconButton(onClick = onClose) { Icon(TalkIcons.Close, "Schließen", tint = Color.White) }
            IconButton(onClick = {
                flash = when (flash) {
                    ImageCapture.FLASH_MODE_OFF -> ImageCapture.FLASH_MODE_AUTO
                    ImageCapture.FLASH_MODE_AUTO -> ImageCapture.FLASH_MODE_ON
                    else -> ImageCapture.FLASH_MODE_OFF
                }
                controller.imageCaptureFlashMode = flash
            }) {
                Icon(
                    when (flash) {
                        ImageCapture.FLASH_MODE_AUTO -> TalkIcons.FlashAuto
                        ImageCapture.FLASH_MODE_ON -> TalkIcons.FlashOn
                        else -> TalkIcons.FlashOff
                    },
                    "Blitz",
                    tint = Color.White,
                )
            }
        }

        Row(
            Modifier
                .align(Alignment.BottomCenter)
                .fillMaxWidth()
                .navigationBarsPadding()
                .padding(bottom = 40.dp),
            horizontalArrangement = Arrangement.SpaceEvenly,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            RoundButton(onClick = onGallery) { Icon(TalkIcons.Gallery, "Galerie", tint = Color.White) }
            // Shutter: white ring with a white disc, like Signal.
            Box(
                Modifier
                    .size(76.dp)
                    .border(4.dp, Color.White, CircleShape)
                    .padding(8.dp)
                    .clip(CircleShape)
                    .background(if (busy) Color.White.copy(alpha = 0.5f) else Color.White)
                    .clickable(enabled = !busy) {
                        busy = true
                        scope.launch {
                            blink.snapTo(0.7f)
                            blink.animateTo(0f, tween(220))
                        }
                        val dir = File(context.cacheDir, "camera").apply { mkdirs() }
                        val file = File.createTempFile("cap", ".jpg", dir)
                        controller.takePicture(
                            ImageCapture.OutputFileOptions.Builder(file).build(),
                            ContextCompat.getMainExecutor(context),
                            object : ImageCapture.OnImageSavedCallback {
                                override fun onImageSaved(output: ImageCapture.OutputFileResults) {
                                    busy = false
                                    onCaptured(Uri.fromFile(file))
                                }

                                override fun onError(e: ImageCaptureException) {
                                    busy = false
                                    file.delete()
                                    Toast.makeText(context, "Foto fehlgeschlagen", Toast.LENGTH_SHORT).show()
                                }
                            },
                        )
                    },
            )
            RoundButton(onClick = {
                front = !front
                controller.cameraSelector = if (front) CameraSelector.DEFAULT_FRONT_CAMERA else CameraSelector.DEFAULT_BACK_CAMERA
            }) { Icon(TalkIcons.SwitchCamera, "Kamera wechseln", tint = Color.White) }
        }
    }
}

@Composable
private fun RoundButton(onClick: () -> Unit, content: @Composable () -> Unit) {
    Box(
        Modifier
            .size(48.dp)
            .clip(CircleShape)
            .background(Color.Black.copy(alpha = 0.35f))
            .clickable(onClick = onClick),
        contentAlignment = Alignment.Center,
    ) { content() }
}
