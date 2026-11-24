package io.github.nibpdf.nib

import android.security.keystore.KeyProperties
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.security.KeyStore
import java.security.PrivateKey
import java.security.Signature
import java.security.interfaces.ECPublicKey
import java.security.spec.MGF1ParameterSpec
import java.security.spec.PSSParameterSpec

/** Signing algorithms supported by the Android KeyStore signer. */
public enum class SigningAlgorithm {
    RsaPss,
    EcdsaP256,
}

/** Digest algorithms used to construct the PDF signature. */
public enum class SigningDigest {
    Sha256,
    Sha384,
    Sha512,
}

/** Signature-widget appearance layers to include in `/AP`. */
public enum class SignatureAppearanceMode {
    Invisible,
    Description,
    Graphic,
    GraphicAndDescription,
}

/** Visible appearance options for an existing signature widget. */
public data class SignatureAppearanceSpec(
    val mode: SignatureAppearanceMode,
    /** Description text shown in the widget's `/n2` layer. */
    val description: String? = null,
    /** UUID of an existing Ink or Stamp annotation whose `/AP` is reused. */
    val graphicRef: String? = null,
    /** Helvetica size in points; supported range is 4..24. */
    val fontSize: Double = 8.0,
)

/** Options for creating an invisible cryptographic PDF signature. */
public data class SignatureSpec(
    /** Existing empty signature field; null creates a new invisible field. */
    val fieldName: String? = null,
    val page: Int = 0,
    val algorithm: SigningAlgorithm = SigningAlgorithm.RsaPss,
    val digest: SigningDigest = SigningDigest.Sha256,
    val reason: String? = null,
    val location: String? = null,
    val contactInfo: String? = null,
    /** Even number of hex characters, excluding `<` and `>`; 0 uses 16384. */
    val contentsHexLength: Int = 0,
    /** Optional layered appearance for an existing, visible signature field. */
    val appearance: SignatureAppearanceSpec? = null,
)

/** The exact data and algorithm requested by Rust for a prepared signature. */
public data class SignatureRequest(
    val data: ByteArray,
    val digest: SigningDigest,
    val algorithm: SigningAlgorithm,
)

/** A raw signature value and its DER certificate chain, leaf first. */
public data class SignatureResult(
    val signature: ByteArray,
    val certificateChain: List<ByteArray>,
)

/** Host signer. Implementations retain custody of all private keys. */
public interface Signer {
    /** DER certificates, leaf first; the first certificate is fixed at prepare. */
    public fun certificateChain(): List<ByteArray>

    /** Sign the returned signed-attributes bytes; this callback runs without Rust locks. */
    public suspend fun sign(request: SignatureRequest): SignatureResult
}

/** Signs with a private key held by AndroidKeyStore. */
public class KeyStoreSigner(
    private val alias: String,
) : Signer {
    private val keyStore: KeyStore =
        KeyStore.getInstance(ANDROID_KEY_STORE).apply { load(null) }

    override fun certificateChain(): List<ByteArray> {
        val chain =
            keyStore.getCertificateChain(alias)
                ?: throw IllegalStateException("AndroidKeyStore has no certificate chain for alias")
        if (chain.isEmpty()) {
            throw IllegalStateException("AndroidKeyStore returned an empty certificate chain")
        }
        return chain.map { it.encoded }
    }

    override suspend fun sign(request: SignatureRequest): SignatureResult =
        withContext(Dispatchers.IO) {
            val certificates = certificateChain()
            val privateKey =
                keyStore.getKey(alias, null) as? PrivateKey
                    ?: throw IllegalStateException("AndroidKeyStore alias has no private key")
            val certificate =
                keyStore.getCertificate(alias)
                    ?: throw IllegalStateException("AndroidKeyStore alias has no certificate")
            val engine =
                when (request.algorithm) {
                    SigningAlgorithm.RsaPss -> {
                        require(certificate.publicKey.algorithm == KeyProperties.KEY_ALGORITHM_RSA) {
                            "RSA-PSS requires an RSA KeyStore key"
                        }
                        Signature.getInstance("SHA${request.digest.bits}withRSA/PSS").apply {
                            setParameter(
                                PSSParameterSpec(
                                    request.digest.jcaName,
                                    "MGF1",
                                    request.digest.mgf1,
                                    request.digest.bits / 8,
                                    1,
                                ),
                            )
                        }
                    }
                    SigningAlgorithm.EcdsaP256 -> {
                        require(request.digest == SigningDigest.Sha256) {
                            "ECDSA P-256 requires SHA-256"
                        }
                        val publicKey =
                            certificate.publicKey as? ECPublicKey
                                ?: throw IllegalArgumentException("ECDSA P-256 requires an EC KeyStore key")
                        require(publicKey.params.curve.field.fieldSize == 256) {
                            "ECDSA-P256 requires a 256-bit EC KeyStore key"
                        }
                        Signature.getInstance("SHA256withECDSA")
                    }
                }
            engine.initSign(privateKey)
            engine.update(request.data)
            SignatureResult(engine.sign(), certificates)
        }

    private val SigningDigest.bits: Int
        get() =
            when (this) {
                SigningDigest.Sha256 -> 256
                SigningDigest.Sha384 -> 384
                SigningDigest.Sha512 -> 512
            }

    private val SigningDigest.jcaName: String
        get() = "SHA-$bits"

    private val SigningDigest.mgf1: MGF1ParameterSpec
        get() =
            when (this) {
                SigningDigest.Sha256 -> MGF1ParameterSpec.SHA256
                SigningDigest.Sha384 -> MGF1ParameterSpec.SHA384
                SigningDigest.Sha512 -> MGF1ParameterSpec.SHA512
            }

    public companion object {
        private const val ANDROID_KEY_STORE = "AndroidKeyStore"
    }
}

/** Raised when PDF signature preparation, completion, or verification fails. */
public class SignatureException(
    message: String,
    cause: Throwable? = null,
) : Exception(message, cause)
