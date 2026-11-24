package io.github.nibpdf.sample

import io.github.nibpdf.nib.NibDocument
import io.github.nibpdf.nib.delta.FlushMode
import io.github.nibpdf.nib.delta.FlushReport
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.io.OutputStream

internal suspend fun saveAfterValidation(
    document: NibDocument,
    optimized: Boolean,
    flatten: Boolean,
    openOutput: suspend () -> OutputStream,
): FlushReport =
    withContext(Dispatchers.IO) {
        val report =
            document.flush(
                if (optimized) FlushMode.FullRewrite else FlushMode.Incremental,
                flatten = flatten,
            )
        val bytes = document.bytes()
        openOutput().use { output -> output.write(bytes) }
        report
    }
