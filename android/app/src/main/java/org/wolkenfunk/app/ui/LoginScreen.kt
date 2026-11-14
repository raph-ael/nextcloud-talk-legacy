package org.wolkenfunk.app.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.systemBarsPadding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.launch
import org.wolkenfunk.app.SessionManager
import uniffi.talk_ffi.CoreException

@Composable
fun LoginScreen(sessions: SessionManager) {
    var server by rememberSaveable { mutableStateOf("") }
    var waiting by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    val colors = LocalTalkColors.current

    fun submit() {
        if (server.isBlank() || waiting) return
        error = null
        waiting = true
        scope.launch {
            try {
                sessions.login(server.trim())
            } catch (e: CoreException.Cancelled) {
                // closed by the user
            } catch (e: CoreException.Network) {
                error = "Server nicht erreichbar. Stimmt die Adresse?"
            } catch (e: CoreException) {
                error = "Anmeldung fehlgeschlagen."
            } finally {
                waiting = false
            }
        }
    }

    Box(Modifier.fillMaxSize().systemBarsPadding().imePadding(), contentAlignment = Alignment.Center) {
        Column(
            Modifier.fillMaxWidth().padding(28.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(14.dp),
        ) {
            Box(Modifier.size(72.dp).clip(RoundedCornerShape(20.dp)).background(colors.accent))
            Text("Willkommen", fontSize = 26.sp, fontWeight = FontWeight.Bold)
            if (waiting) {
                Text(
                    "Bestätige die Anmeldung im Browser und kehre dann hierher zurück.",
                    color = colors.muted,
                    textAlign = TextAlign.Center,
                )
                CircularProgressIndicator()
                TextButton(onClick = { sessions.cancelLogin() }) { Text("Abbrechen") }
            } else {
                Text("Melde dich mit deinem Nextcloud-Konto an.", color = colors.muted, textAlign = TextAlign.Center)
                Spacer(Modifier.height(4.dp))
                OutlinedTextField(
                    value = server,
                    onValueChange = { server = it },
                    placeholder = { Text("cloud.example.org") },
                    singleLine = true,
                    shape = RoundedCornerShape(14.dp),
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri, imeAction = ImeAction.Go),
                    keyboardActions = KeyboardActions(onGo = { submit() }),
                    modifier = Modifier.fillMaxWidth(),
                )
                Button(
                    onClick = ::submit,
                    enabled = server.isNotBlank(),
                    shape = RoundedCornerShape(14.dp),
                    modifier = Modifier.fillMaxWidth().height(50.dp),
                ) { Text("Anmelden", fontWeight = FontWeight.SemiBold) }
                error?.let { Text(it, color = MaterialTheme.colorScheme.error, textAlign = TextAlign.Center) }
            }
        }
    }
}
