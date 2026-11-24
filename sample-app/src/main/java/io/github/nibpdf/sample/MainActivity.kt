package io.github.nibpdf.sample

import android.content.Intent
import android.graphics.BitmapFactory
import android.net.Uri
import android.os.Bundle
import android.util.Log
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import io.github.nibpdf.nib.NibDocument
import io.github.nibpdf.nib.delta.DeltaIds
import io.github.nibpdf.nib.delta.DeltaOps
import io.github.nibpdf.nib.delta.InvalidDeltaException
import io.github.nibpdf.nib.delta.SignedDocumentException
import io.github.nibpdf.nib.ui.NibPdfView
import io.github.nibpdf.nib.ui.StampSpec
import io.github.nibpdf.nib.ui.VisualSignatureRequest
import io.github.nibpdf.nib.ui.rememberNibPdfViewState
import kotlinx.coroutines.CompletableDeferred
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
                    decodeStamp = { uri -> decodeStamp(contentResolver, uri) },
                    onShare = { json ->
                        val send =
                            Intent(Intent.ACTION_SEND)
                                .setType("application/json")
                                .putExtra(Intent.EXTRA_TEXT, json)
                        startActivity(Intent.createChooser(send, "Export annotations"))
                    },
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
internal fun SampleScreen(
    openAsset: suspend (String) -> ByteArray,
    openUri: suspend (Uri) -> ByteArray?,
    decodeStamp: suspend (Uri) -> android.graphics.Bitmap?,
    onShare: (String) -> Unit,
    openDocument: suspend (ByteArray) -> NibDocument = { NibDocument.open(it) },
) {
    var document by remember { mutableStateOf<NibDocument?>(null) }
    var openedUri by remember { mutableStateOf<Uri?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    val context = LocalContext.current
    val activity = context as? MainActivity
    var textRequest by remember { mutableStateOf<Pair<String, CompletableDeferred<String?>>?>(null) }
    var lastSignature by remember { mutableStateOf<CapturedSignature?>(null) }

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
                        document = openDocument(bytes)
                        openedUri = uri
                    } catch (t: Throwable) {
                        error = t.message ?: t.toString()
                    }
                }
            }
        }

    // Open a bundled fixture when launched with --es nib.asset <name>.
    LaunchedEffect(Unit) {
        activity?.assetIntent?.let { name ->
            try {
                document = openDocument(openAsset(name))
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
                val revision by doc.revision.collectAsState()
                val signaturePresent =
                    remember(doc, revision) {
                        doc.formFields().any {
                            it.fieldType == io.github.nibpdf.nib.delta.FieldType.Signature &&
                                it.signatureState == io.github.nibpdf.nib.delta.SignatureFieldState.Occupied
                        }
                    }

                if (signaturePresent) {
                    Text(
                        "Signature present. Integrity and trust have not been evaluated.",
                        Modifier.fillMaxWidth().testTag("signatureStatusBadge"),
                    )
                }
                val state = rememberNibPdfViewState(doc)
                var signatureRequest by remember(doc) { mutableStateOf<VisualSignatureRequest?>(null) }
                var pickedSignatureImage by remember(doc) { mutableStateOf<android.graphics.Bitmap?>(null) }
                var imagePickOwner by remember(doc) { mutableStateOf<VisualSignatureRequest?>(null) }
                val requestVisualSignature =
                    remember(doc) {
                        { request: VisualSignatureRequest ->
                            pickedSignatureImage = null
                            signatureRequest = request
                        }
                    }
                val bundledStamp =
                    remember {
                        context.assets.open("stamp.png").use { BitmapFactory.decodeStream(it) }
                    }

                fun toast(msg: String) {
                    Toast.makeText(context, msg, Toast.LENGTH_SHORT).show()
                }

                val stampPicker =
                    rememberLauncherForActivityResult(
                        ActivityResultContracts.PickVisualMedia(),
                    ) { uri ->
                        if (uri != null) {
                            scope.launch {
                                val bmp = withContext(Dispatchers.IO) { decodeStamp(uri) }
                                if (bmp != null) {
                                    state.pendingStamp =
                                        StampSpec.Image(
                                            png = bmp.toPng(),
                                            widthPx = bmp.width,
                                            heightPx = bmp.height,
                                        )
                                    state.tool = io.github.nibpdf.nib.ui.Tool.Stamp
                                }
                            }
                        }
                    }
                val signatureImagePicker =
                    rememberLauncherForActivityResult(
                        ActivityResultContracts.PickVisualMedia(),
                    ) { uri ->
                        val owner = imagePickOwner
                        imagePickOwner = null
                        if (uri != null && owner != null && owner === signatureRequest) {
                            scope.launch {
                                try {
                                    val bitmap = withContext(Dispatchers.IO) { decodeStamp(uri) }
                                    if (owner === signatureRequest) {
                                        if (bitmap == null) {
                                            toast("Could not decode the selected signature image")
                                        } else {
                                            pickedSignatureImage = bitmap
                                        }
                                    }
                                } catch (e: java.io.IOException) {
                                    toast("Could not read the selected image: ${e.message}")
                                } catch (e: SecurityException) {
                                    toast("Cannot access the selected image: ${e.message}")
                                } catch (e: IllegalArgumentException) {
                                    toast("Invalid signature image: ${e.message}")
                                } catch (e: IllegalStateException) {
                                    toast("Could not encode the selected image: ${e.message}")
                                }
                            }
                        }
                    }

                state.author = "Sample"
                state.onRequestVisualSignature = requestVisualSignature
                state.onRequestText = { initial ->
                    val d = CompletableDeferred<String?>()
                    textRequest = initial to d
                    d.await()
                }

                suspend fun saveToUri(
                    d: NibDocument,
                    uri: Uri,
                    optimized: Boolean,
                    flatten: Boolean = false,
                ): io.github.nibpdf.nib.delta.FlushReport =
                    saveAfterValidation(d, optimized, flatten) {
                        context.contentResolver.openOutputStream(uri, "wt")
                            ?: throw IllegalStateException("no output stream for $uri")
                    }

                fun savedMessage(r: io.github.nibpdf.nib.delta.FlushReport): String =
                    "Saved rev ${r.revision} (${r.annotsWritten} annots, " +
                        "${r.fieldsWritten} fields, ${r.bytesWritten} bytes" +
                        "${if (r.flattened) ", flattened" else ""})"

                val saveAsLauncher =
                    rememberLauncherForActivityResult(
                        ActivityResultContracts.CreateDocument("application/pdf"),
                    ) { uri ->
                        if (uri != null) {
                            scope.launch {
                                try {
                                    val report =
                                        try {
                                            saveToUri(doc, uri, optimized = true)
                                        } catch (e: SignedDocumentException) {
                                            saveToUri(doc, uri, optimized = false).also {
                                                toast("Signed document: saved unoptimized")
                                            }
                                        }
                                    toast(savedMessage(report))
                                } catch (t: Throwable) {
                                    toast("Save failed: ${t.message ?: t.toString()}")
                                }
                            }
                        }
                    }

                val saveAsFlatLauncher =
                    rememberLauncherForActivityResult(
                        ActivityResultContracts.CreateDocument("application/pdf"),
                    ) { uri ->
                        if (uri != null) {
                            scope.launch {
                                try {
                                    toast(
                                        savedMessage(
                                            saveToUri(doc, uri, optimized = true, flatten = true),
                                        ),
                                    )
                                } catch (t: Throwable) {
                                    toast("Save failed: ${t.message ?: t.toString()}")
                                }
                            }
                        }
                    }

                NibPdfView(state, Modifier.weight(1f).fillMaxWidth())
                ToolPalette(
                    state = state,
                    bundledStamp = bundledStamp,
                    saveEnabled = openedUri != null,
                    onSave = {
                        val uri = openedUri ?: return@ToolPalette
                        scope.launch {
                            try {
                                toast(savedMessage(saveToUri(doc, uri, optimized = false)))
                            } catch (t: Throwable) {
                                toast("Save failed: ${t.message ?: t.toString()}")
                            }
                        }
                    },
                    onSaveAs = { saveAsLauncher.launch("document.pdf") },
                    onSaveAsFlat = { saveAsFlatLauncher.launch("document-flattened.pdf") },
                    onPickStamp = {
                        stampPicker.launch(
                            PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageOnly),
                        )
                    },
                    onExport = {
                        val json = doc.exportDelta(NibDocument.ExportMode.Compacted)
                        Log.i("nib", json.take(2000))
                        onShare(json)
                    },
                )
                signatureRequest?.let { request ->
                    SignatureCaptureDialog(
                        previous = lastSignature,
                        pickedImage = pickedSignatureImage,
                        onPickImage = {
                            imagePickOwner = request
                            signatureImagePicker.launch(
                                PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageOnly),
                            )
                        },
                        onDismiss = {
                            signatureRequest = null
                            pickedSignatureImage = null
                        },
                        onCaptureError = { message -> toast("Could not capture visual signature: $message") },
                        onApply = { captured ->
                            try {
                                doc.apply(
                                    DeltaOps.create(
                                        DeltaIds.uuidV4(),
                                        request.page,
                                        signaturePayload(request, captured),
                                        state.author,
                                    ),
                                )
                                lastSignature = captured
                                signatureRequest = null
                                pickedSignatureImage = null
                                toast("Visual mark added; the field is not digitally signed")
                            } catch (e: InvalidDeltaException) {
                                toast("Could not add visual signature: ${e.message}")
                            } catch (e: uniffi.nib_ffi.NibException) {
                                toast("Could not access the PDF: ${e.message}")
                            }
                        },
                    )
                }
            }
        }
    }

    textRequest?.let { (initial, deferred) ->
        var text by remember { mutableStateOf(initial) }
        AlertDialog(
            onDismissRequest = {
                textRequest = null
                deferred.complete(null)
            },
            title = { Text("Text") },
            text = {
                OutlinedTextField(
                    value = text,
                    onValueChange = { text = it },
                    modifier = Modifier.testTag("textField"),
                )
            },
            confirmButton = {
                TextButton(
                    onClick = {
                        textRequest = null
                        deferred.complete(text)
                    },
                    modifier = Modifier.testTag("textOk"),
                ) { Text("OK") }
            },
            dismissButton = {
                TextButton(
                    onClick = {
                        textRequest = null
                        deferred.complete(null)
                    },
                ) { Text("Cancel") }
            },
        )
    }
}
