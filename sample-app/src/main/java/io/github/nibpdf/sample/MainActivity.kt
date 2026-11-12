package io.github.nibpdf.sample

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import io.github.nibpdf.nib.Nib

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val pdfium = runCatching { Nib.pdfiumVersion() }.getOrElse { "ERROR: $it" }
        setContent {
            MaterialTheme {
                Column(Modifier.fillMaxSize().padding(24.dp)) {
                    Text("nib ${Nib.version()}", Modifier.testTag("nibVersion"))
                    Text(pdfium, Modifier.testTag("pdfiumVersion"))
                }
            }
        }
    }
}
