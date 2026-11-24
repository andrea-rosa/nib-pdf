package io.github.nibpdf.nib

import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import androidx.test.ext.junit.runners.AndroidJUnit4
import io.github.nibpdf.nib.delta.DeltaIds
import io.github.nibpdf.nib.delta.DeltaOps
import io.github.nibpdf.nib.delta.SignatureFieldState
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.add
import kotlinx.serialization.json.buildJsonArray
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import java.security.KeyPairGenerator
import java.security.KeyStore
import java.security.spec.ECGenParameterSpec

@RunWith(AndroidJUnit4::class)
class KeyStoreSignTest {
    @Test
    fun ecdsaKeyStoreKeySignsThroughPublicFacade() =
        runBlocking {
            withEcdsaSigner { signer ->
                val doc = NibDocument.open(assetBytes("sig-field.pdf"))
                try {
                    val report =
                        doc.sign(
                            SignatureSpec(
                                fieldName = "Signature1",
                                algorithm = SigningAlgorithm.EcdsaP256,
                            ),
                            signer,
                        )
                    assertTrue(report.bytesWritten > 0)
                    assertEquals(
                        SignatureFieldState.Occupied,
                        doc.formField("Signature1")?.signatureState,
                    )

                    val reopened = NibDocument.open(doc.bytes())
                    try {
                        assertEquals(
                            SignatureFieldState.Occupied,
                            reopened.formField("Signature1")?.signatureState,
                        )
                    } finally {
                        reopened.close()
                    }
                } finally {
                    doc.close()
                }
            }
        }

    @Test
    fun rsaPssKeyStoreKeySignsThroughPublicFacade() =
        runBlocking {
            withRsaSigner { signer ->
                val doc = NibDocument.open(assetBytes("sig-field.pdf"))
                try {
                    val report =
                        doc.sign(
                            SignatureSpec(fieldName = "Signature1", algorithm = SigningAlgorithm.RsaPss),
                            signer,
                        )
                    assertTrue(report.bytesWritten > 0)
                    assertEquals(
                        SignatureFieldState.Occupied,
                        NibDocument.open(doc.bytes()).use { it.formField("Signature1")?.signatureState },
                    )
                } finally {
                    doc.close()
                }
            }
        }

    @Test
    fun invalidSignatureIsRejectedWithoutPublishingStagedBytes() =
        runBlocking {
            withEcdsaSigner { signer ->
                val doc = NibDocument.open(assetBytes("sig-field.pdf"))
                try {
                    val original = doc.bytes()
                    var failure: SignatureException? = null
                    try {
                        doc.sign(
                            SignatureSpec(fieldName = "Signature1", algorithm = SigningAlgorithm.EcdsaP256),
                            object : Signer {
                                override fun certificateChain(): List<ByteArray> = signer.certificateChain()

                                override suspend fun sign(request: SignatureRequest): SignatureResult =
                                    SignatureResult(byteArrayOf(1), certificateChain())
                            },
                        )
                    } catch (e: SignatureException) {
                        failure = e
                    }
                    assertNotNull(failure)
                    assertTrue(original.contentEquals(doc.bytes()))
                    assertEquals(SignatureFieldState.Empty, doc.formField("Signature1")?.signatureState)
                } finally {
                    doc.close()
                }
            }
        }

    @Test
    fun hostSignerFailureAbortsPreparation() =
        runBlocking {
            withEcdsaSigner { signer ->
                val doc = NibDocument.open(assetBytes("sig-field.pdf"))
                try {
                    val original = doc.bytes()
                    var failure: IllegalStateException? = null
                    try {
                        doc.sign(
                            SignatureSpec(fieldName = "Signature1", algorithm = SigningAlgorithm.EcdsaP256),
                            object : Signer {
                                override fun certificateChain(): List<ByteArray> = signer.certificateChain()

                                override suspend fun sign(request: SignatureRequest): SignatureResult =
                                    throw IllegalStateException("host signer cancelled")
                            },
                        )
                    } catch (e: IllegalStateException) {
                        failure = e
                    }
                    assertEquals("host signer cancelled", failure?.message)
                    assertTrue(original.contentEquals(doc.bytes()))
                    assertEquals(SignatureFieldState.Empty, doc.formField("Signature1")?.signatureState)
                } finally {
                    doc.close()
                }
            }
        }

    @Test
    fun concurrentEditDuringHostSignerRejectsAdoption() =
        runBlocking {
            withEcdsaSigner { signer ->
                val doc = NibDocument.open(assetBytes("sig-field.pdf"))
                try {
                    val original = doc.bytes()
                    var failure: SignatureException? = null
                    try {
                        doc.sign(
                            SignatureSpec(fieldName = "Signature1", algorithm = SigningAlgorithm.EcdsaP256),
                            object : Signer {
                                override fun certificateChain(): List<ByteArray> = signer.certificateChain()

                                override suspend fun sign(request: SignatureRequest): SignatureResult {
                                    val result = signer.sign(request)
                                    doc.apply(
                                        DeltaOps.create(
                                            DeltaIds.uuidV4(),
                                            0,
                                            buildJsonObject {
                                                put("subtype", "Square")
                                                put("page", 0)
                                                put(
                                                    "rect",
                                                    buildJsonArray {
                                                        add(10)
                                                        add(10)
                                                        add(50)
                                                        add(50)
                                                    },
                                                )
                                                put("color", "#FF0000")
                                            },
                                        ),
                                    )
                                    return result
                                }
                            },
                        )
                    } catch (e: SignatureException) {
                        failure = e
                    }
                    assertNotNull(failure)
                    assertTrue(original.contentEquals(doc.bytes()))
                    assertEquals(SignatureFieldState.Empty, doc.formField("Signature1")?.signatureState)
                    assertEquals(1, doc.annotations(0).size)
                } finally {
                    doc.close()
                }
            }
        }

    private suspend fun withEcdsaSigner(block: suspend (KeyStoreSigner) -> Unit) {
        val alias = "nib-sign-${System.currentTimeMillis()}"
        generateEcdsaP256Key(alias)
        try {
            block(KeyStoreSigner(alias))
        } finally {
            KeyStore.getInstance("AndroidKeyStore").apply { load(null) }.deleteEntry(alias)
        }
    }

    private suspend fun withRsaSigner(block: suspend (KeyStoreSigner) -> Unit) {
        val alias = "nib-sign-rsa-${System.currentTimeMillis()}"
        generateRsaPssKey(alias)
        try {
            block(KeyStoreSigner(alias))
        } finally {
            KeyStore.getInstance("AndroidKeyStore").apply { load(null) }.deleteEntry(alias)
        }
    }

    private fun generateEcdsaP256Key(alias: String) {
        KeyPairGenerator.getInstance(KeyProperties.KEY_ALGORITHM_EC, ANDROID_KEY_STORE).apply {
            initialize(
                KeyGenParameterSpec
                    .Builder(
                        alias,
                        KeyProperties.PURPOSE_SIGN or KeyProperties.PURPOSE_VERIFY,
                    ).setDigests(KeyProperties.DIGEST_SHA256)
                    .setAlgorithmParameterSpec(ECGenParameterSpec("secp256r1"))
                    .build(),
            )
            generateKeyPair()
        }
    }

    private fun generateRsaPssKey(alias: String) {
        KeyPairGenerator.getInstance(KeyProperties.KEY_ALGORITHM_RSA, ANDROID_KEY_STORE).apply {
            initialize(
                KeyGenParameterSpec
                    .Builder(
                        alias,
                        KeyProperties.PURPOSE_SIGN or KeyProperties.PURPOSE_VERIFY,
                    ).setDigests(
                        KeyProperties.DIGEST_SHA256,
                        KeyProperties.DIGEST_SHA384,
                        KeyProperties.DIGEST_SHA512,
                    ).setSignaturePaddings(KeyProperties.SIGNATURE_PADDING_RSA_PSS)
                    .setKeySize(2048)
                    .build(),
            )
            generateKeyPair()
        }
    }

    private companion object {
        const val ANDROID_KEY_STORE = "AndroidKeyStore"
    }
}
