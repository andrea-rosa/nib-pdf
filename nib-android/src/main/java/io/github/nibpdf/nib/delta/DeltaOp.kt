package io.github.nibpdf.nib.delta

import android.graphics.RectF
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import java.security.SecureRandom
import java.time.Instant
import java.time.ZoneOffset
import java.time.format.DateTimeFormatter
import java.util.UUID

/** Shared JSON codec for the delta wire format (JSON_SCHEMA §2–3). */
internal val NibJson: Json =
    Json {
        ignoreUnknownKeys = true
        explicitNulls = false
        encodeDefaults = false
    }

/** A single delta operation (JSON_SCHEMA §3). */
@Serializable
public data class DeltaOp(
    val opId: String,
    val ts: String,
    val author: String? = null,
    val groupId: String? = null,
    @SerialName("type") val kind: OpKind,
    val target: OpTarget,
    val id: String,
    val page: Int? = null,
    val payload: JsonObject,
)

@Serializable
public enum class OpKind {
    @SerialName("create")
    Create,

    @SerialName("update")
    Update,

    @SerialName("delete")
    Delete,
}

@Serializable
public enum class OpTarget {
    @SerialName("annotation")
    Annotation,

    @SerialName("formField")
    FormField,

    @SerialName("signature")
    Signature,
}

/** Delta export/import envelope (JSON_SCHEMA §2). */
@Serializable
public data class DeltaDocument(
    val schemaVersion: Int,
    val documentId: String,
    val pdfId: List<String>? = null,
    val baseRevision: Int,
    val generator: String? = null,
    val createdAt: String? = null,
    val ops: List<DeltaOp>,
)

/** One validation failure inside a rejected delta (JSON_SCHEMA §3). */
@Serializable
public data class DeltaValidationError(
    val opIndex: Int,
    val path: String,
    val code: String,
    val message: String,
)

/** Dirty page rect from an apply/import (user space). */
public data class DirtyRegion(
    val page: Int,
    val rect: RectF,
)

/** Result of an apply/import. */
public data class ApplyReport(
    val applied: List<String>,
    val skipped: List<String>,
    val rejected: List<DeltaValidationError>,
    val dirty: List<DirtyRegion>,
)

/** Thrown when a delta fails validation; [errors] is the full list. */
public class InvalidDeltaException(
    public val errors: List<DeltaValidationError>,
) : Exception("invalid delta: ${errors.joinToString { "${it.code} at ${it.path}" }}")

/** Thrown when an import's `baseRevision` doesn't match the engine's. */
public class RevisionMismatchException(
    public val expected: Int,
    public val got: Int,
) : Exception("revision mismatch: expected $expected, got $got")

/** UUID generation (JSON_SCHEMA §3: `opId`/`groupId` are UUIDv7). */
public object DeltaIds {
    private val random = SecureRandom()

    /**
     * UUIDv7: 48-bit Unix-epoch milliseconds (big-endian) + 74 random bits,
     * version nibble 7, variant `10`.
     */
    public fun uuidV7(): String {
        val ms = System.currentTimeMillis()
        val b = ByteArray(16)
        b[0] = (ms ushr 40).toByte()
        b[1] = (ms ushr 32).toByte()
        b[2] = (ms ushr 24).toByte()
        b[3] = (ms ushr 16).toByte()
        b[4] = (ms ushr 8).toByte()
        b[5] = ms.toByte()
        val tail = ByteArray(10)
        random.nextBytes(tail)
        System.arraycopy(tail, 0, b, 6, 10)
        b[6] = (b[6].toInt() and 0x0F or 0x70).toByte()
        b[8] = (b[8].toInt() and 0x3F or 0x80).toByte()
        val hi =
            (0 until 8).fold(0L) { acc, i -> (acc shl 8) or (b[i].toLong() and 0xFF) }
        val lo =
            (8 until 16).fold(0L) { acc, i -> (acc shl 8) or (b[i].toLong() and 0xFF) }
        return UUID(hi, lo).toString()
    }

    public fun uuidV4(): String = UUID.randomUUID().toString()
}

/** RFC 3339 / ISO-8601 UTC timestamps with millisecond precision. */
public object Rfc3339 {
    private val formatter =
        DateTimeFormatter.ofPattern("yyyy-MM-dd'T'HH:mm:ss.SSS'Z'").withZone(ZoneOffset.UTC)

    /** e.g. `"2026-11-13T10:00:00.123Z"`. */
    public fun now(): String = formatter.format(Instant.now().truncatedTo(java.time.temporal.ChronoUnit.MILLIS))
}

/** Op constructors: fill `opId` (v7), `ts`, `target = annotation`. */
public object DeltaOps {
    public fun create(
        id: String,
        page: Int,
        payload: JsonObject,
        author: String? = null,
        groupId: String? = null,
    ): DeltaOp =
        DeltaOp(
            opId = DeltaIds.uuidV7(),
            ts = Rfc3339.now(),
            author = author,
            groupId = groupId,
            kind = OpKind.Create,
            target = OpTarget.Annotation,
            id = id,
            page = page,
            payload = payload,
        )

    public fun update(
        id: String,
        patch: JsonObject,
        page: Int? = null,
        author: String? = null,
        groupId: String? = null,
    ): DeltaOp =
        DeltaOp(
            opId = DeltaIds.uuidV7(),
            ts = Rfc3339.now(),
            author = author,
            groupId = groupId,
            kind = OpKind.Update,
            target = OpTarget.Annotation,
            id = id,
            page = page,
            payload = patch,
        )

    public fun delete(
        id: String,
        page: Int? = null,
        author: String? = null,
        groupId: String? = null,
    ): DeltaOp =
        DeltaOp(
            opId = DeltaIds.uuidV7(),
            ts = Rfc3339.now(),
            author = author,
            groupId = groupId,
            kind = OpKind.Delete,
            target = OpTarget.Annotation,
            id = id,
            page = page,
            payload = JsonObject(emptyMap()),
        )
}
