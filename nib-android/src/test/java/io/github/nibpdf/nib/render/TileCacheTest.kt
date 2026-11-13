package io.github.nibpdf.nib.render

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

private data class FakeTile(
    val bytes: Int,
)

class TileCacheTest {
    private fun cache(
        maxBytes: Long,
        evicted: MutableList<FakeTile> = mutableListOf(),
    ) = TileCache<Int, FakeTile>(maxBytes, { it.bytes }, { evicted.add(it) })

    @Test
    fun byteBudgetIsRespected() {
        val c = cache(100)
        repeat(5) { c.put(it, FakeTile(40)) }
        // At most 2 × 40-byte tiles fit.
        var total = 0
        for (i in 0 until 5) c[i]?.let { total += it.bytes }
        assertTrue(total <= 80)
    }

    @Test
    fun lruOrderEvictsLeastRecentlyUsed() {
        val evicted = mutableListOf<FakeTile>()
        val c = cache(80, evicted)
        c.put(0, FakeTile(40))
        c.put(1, FakeTile(40))
        c[0] // touch key 0 so key 1 becomes LRU
        c.put(2, FakeTile(40))
        assertNotNull(c[0])
        assertNull(c[1])
        assertNotNull(c[2])
        assertEquals(1, evicted.size)
    }

    @Test
    fun evictedEntriesGoToOnEvict() {
        val evicted = mutableListOf<FakeTile>()
        val c = cache(40, evicted)
        c.put(0, FakeTile(40))
        c.put(1, FakeTile(40))
        assertEquals(1, evicted.size)
    }

    @Test
    fun trimLevels() {
        val c = cache(400)
        repeat(4) { c.put(it, FakeTile(100)) }
        c.trim(TileCache.TRIM_MEMORY_RUNNING_MODERATE)
        assertTrue(c.size() <= 200)
        c.trim(TileCache.TRIM_MEMORY_RUNNING_LOW)
        assertEquals(0, c.size())
    }

    @Test
    fun evictMatching() {
        val c = cache(400)
        repeat(4) { c.put(it, FakeTile(100)) }
        c.evictMatching { it < 2 }
        assertNull(c[2])
        assertNull(c[3])
        assertNotNull(c[0])
        assertNotNull(c[1])
    }

    @Test
    fun defaultBudgetWithinBounds() {
        val b = TileCache.defaultBudget()
        assertTrue(b > 0)
        assertTrue(b <= 96L * 1024 * 1024)
    }
}
