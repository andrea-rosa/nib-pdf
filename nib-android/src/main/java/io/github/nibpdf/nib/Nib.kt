package io.github.nibpdf.nib

/** Entry point of the Nib PDF library (M0: version probes only). */
public object Nib {
    init {
        // UniFFI invokes this callback on Rust threads (the nib-render worker
        // wakes the future from there). JNA's default detaches the thread
        // after the callback returns, which ART aborts on ("attempting to
        // detach while still running code") because the coroutine resume
        // leaves Java frames on the stack. Keep the thread attached instead.
        com.sun.jna.Native.setCallbackThreadInitializer(
            uniffi.nib_ffi.uniffiRustFutureContinuationCallbackImpl,
            // daemon = true, detach = false
            com.sun.jna.CallbackThreadInitializer(true, false),
        )
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
