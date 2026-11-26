package io.github.nibpdf.sample

import android.content.Intent
import io.github.nibpdf.nib.NibDocument
import io.github.nibpdf.nib.delta.DeltaIds
import io.github.nibpdf.nib.delta.DeltaOps
import io.github.nibpdf.nib.ui.Tool
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonArray
import kotlinx.serialization.json.buildJsonObject

/**
 * Launch extras used by `:macrobenchmark` (see macrobenchmark/README.md):
 * - `nib.synthetic` = `text:<MB>` opens a generated [SyntheticPdf];
 * - `nib.tool` = a [Tool] name preselected once the view appears;
 * - `nib.bench.flushOps` = N creates N Square annotations and flushes once
 *   before the document is shown.
 */
internal data class BenchmarkLaunch(
    val synthetic: String?,
    val tool: Tool?,
    val flushOps: Int,
) {
    companion object {
        fun from(intent: Intent): BenchmarkLaunch =
            BenchmarkLaunch(
                synthetic = intent.getStringExtra("nib.synthetic"),
                tool = intent.getStringExtra("nib.tool")?.let { name -> Tool.entries.firstOrNull { it.name == name } },
                flushOps = intent.getIntExtra("nib.bench.flushOps", 0).coerceIn(0, 1000),
            )
    }
}

/** Applies [count] Square creates spread over the pages, then one incremental flush. */
internal fun applyAndFlush(
    doc: NibDocument,
    count: Int,
) {
    val ops =
        (0 until count).map { i ->
            val page = i % doc.pageCount
            val x = 40.0 + (i % 10) * 50.0
            val y = 60.0 + (i / 10 % 10) * 60.0
            DeltaOps.create(
                DeltaIds.uuidV4(),
                page,
                buildJsonObject {
                    put("subtype", JsonPrimitive("Square"))
                    put("page", JsonPrimitive(page))
                    put(
                        "rect",
                        buildJsonArray {
                            listOf(x, y, x + 40.0, y + 40.0).forEach { add(JsonPrimitive(it)) }
                        },
                    )
                    put("color", JsonPrimitive("#FF0000"))
                },
                "Benchmark",
            )
        }
    doc.apply(ops)
    doc.flush()
}
