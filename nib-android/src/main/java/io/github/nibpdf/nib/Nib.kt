package io.github.nibpdf.nib

/** Entry point of the Nib PDF library (M0: version probes only). */
public object Nib {
    init {
        // Load PDFium first so the dynamic linker already has it mapped when
        // libnib_ffi dlopens it.
        System.loadLibrary("pdfium")
        System.loadLibrary("nib_ffi")
    }

    /** Version of the Rust core (`nib-ffi` Cargo package version). */
    public fun version(): String = uniffi.nib_ffi.`nibVersion`()

    /**
     * Probe string proving PDFium was loaded and callable; throws
     * `uniffi.nib_ffi.NibException` on failure.
     */
    public fun pdfiumVersion(): String = uniffi.nib_ffi.`pdfiumVersion`()
}
