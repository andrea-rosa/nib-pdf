package io.github.nibpdf.nib

import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class NibSmokeTest {
    @Test
    fun versionsAreReported() {
        assertEquals(BuildConfig.NIB_FFI_VERSION, Nib.version())
        val p = Nib.pdfiumVersion()
        assertTrue(p.isNotEmpty())
        assertTrue(p, p.contains("pdfium ok"))
    }
}
