package org.sirplus.talk

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import org.sirplus.talk.ui.TalkApp
import org.sirplus.talk.ui.TalkTheme

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        val sessions = (application as TalkApplication).sessions
        setContent {
            TalkTheme { TalkApp(sessions) }
        }
    }
}
