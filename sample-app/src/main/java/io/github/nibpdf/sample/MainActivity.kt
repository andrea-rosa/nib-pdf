package io.github.nibpdf.sample

import android.net.Uri
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import io.github.nibpdf.nib.NibDocument
import io.github.nibpdf.nib.ui.NibPdfView
import io.github.nibpdf.nib.ui.rememberNibPdfViewState
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent {
            MaterialTheme {
                SampleScreen(
                    openAsset = { name -> assets.open(name).readBytes() },
                    openUri = { uri -> contentResolver.openInputStream(uri)?.readBytes() },
                )
            }
        }
        intent.getStringExtra("nib.asset")?.let { assetIntent = it }
    }

    // Consumed by SampleScreen via a CompositionLocal-free handoff: the
    // activity stores it and the composable reads it once.
    var assetIntent: String? = null
        internal set
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
@Suppress("ktlint:standard:function-naming")
private fun SampleScreen(
    openAsset: suspend (String) -> ByteArray,
    openUri: suspend (Uri) -> ByteArray?,
) {
    var document by remember { mutableStateOf<NibDocument?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    val activity = androidx.compose.ui.platform.LocalContext.current as? MainActivity

    val picker =
        rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
            if (uri != null) {
                scope.launch {
                    error = null
                    try {
                        val bytes =
                            withContext(Dispatchers.IO) { openUri(uri) }
                                ?: throw IllegalStateException("empty stream")
                        document?.close()
                        document = NibDocument.open(bytes)
                    } catch (t: Throwable) {
                        error = t.message ?: t.toString()
                    }
                }
            }
        }

    // Open a bundled fixture when launched with --es nib.asset <name>.
    androidx.compose.runtime.LaunchedEffect(Unit) {
        activity?.assetIntent?.let { name ->
            try {
                document = NibDocument.open(openAsset(name))
            } catch (t: Throwable) {
                error = t.message ?: t.toString()
            }
            activity.assetIntent = null
        }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = {
                    Text(
                        document?.let { "Nib — ${it.pageCount} pages" } ?: "Nib",
                        Modifier.testTag("title"),
                    )
                },
                actions = {
                    Button(
                        onClick = { picker.launch(arrayOf("application/pdf")) },
                        modifier = Modifier.testTag("openButton"),
                    ) {
                        Text("Open")
                    }
                },
            )
        },
    ) { padding ->
        Column(Modifier.fillMaxSize().padding(padding)) {
            error?.let {
                Text(it, Modifier.fillMaxWidth().testTag("error"))
            }
            document?.let { doc ->
                NibPdfView(rememberNibPdfViewState(doc), Modifier.fillMaxSize())
            }
        }
    }
}
