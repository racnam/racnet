package org.racnet.android.policy

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.racnet.android.policy.DuplicateLinkPolicy.Link

class DuplicateLinkPolicyTest {

    private val smaller = ByteArray(32) { 0x11 }
    private val larger = ByteArray(32) { 0x22 }

    @Test
    fun `single links are never victims`() {
        assertTrue(
            DuplicateLinkPolicy.victims(smaller, larger, listOf(Link(0uL, true))).isEmpty(),
        )
        assertTrue(DuplicateLinkPolicy.victims(smaller, larger, emptyList()).isEmpty())
    }

    @Test
    fun `the smaller fingerprint's link survives`() {
        // We are smaller: keep the link we initiated.
        val victims = DuplicateLinkPolicy.victims(
            smaller,
            larger,
            listOf(Link(4uL, weInitiated = true), Link(9uL, weInitiated = false)),
        )
        assertEquals(listOf(9uL), victims)

        // We are larger: keep the link the (smaller) peer initiated.
        val victims2 = DuplicateLinkPolicy.victims(
            larger,
            smaller,
            listOf(Link(4uL, weInitiated = true), Link(9uL, weInitiated = false)),
        )
        assertEquals(listOf(4uL), victims2)
    }

    @Test
    fun `both sides close the same physical link`() {
        // One physical situation, seen from both ends. A (smaller)
        // initiated link α; B (larger) initiated link β. A sees α as
        // weInitiated and β as accepted; B sees the mirror image.
        val aVictims = DuplicateLinkPolicy.victims(
            smaller,
            larger,
            listOf(Link(0uL, weInitiated = true), Link(1uL, weInitiated = false)),
        )
        val bVictims = DuplicateLinkPolicy.victims(
            larger,
            smaller,
            listOf(Link(10uL, weInitiated = false), Link(11uL, weInitiated = true)),
        )
        // A keeps its initiated link (0, which is α) and closes 1 (β);
        // B keeps its accepted link (10, which is α) and closes 11 (β).
        assertEquals(listOf(1uL), aVictims)
        assertEquals(listOf(11uL), bVictims)
    }

    @Test
    fun `equal fingerprints mean self-talk and close everything`() {
        val victims = DuplicateLinkPolicy.victims(
            smaller,
            smaller.copyOf(),
            listOf(Link(0uL, true), Link(1uL, false)),
        )
        assertEquals(setOf(0uL, 1uL), victims.toSet())
    }

    @Test
    fun `extra duplicates collapse to one deterministic keeper`() {
        val victims = DuplicateLinkPolicy.victims(
            smaller,
            larger,
            listOf(
                Link(7uL, weInitiated = true),
                Link(3uL, weInitiated = true),
                Link(9uL, weInitiated = false),
            ),
        )
        // Keeper is the lowest-id link we initiated (3): the rest close.
        assertEquals(setOf(7uL, 9uL), victims.toSet())
    }

    @Test
    fun `fingerprint comparison is bytewise unsigned`() {
        val high = byteArrayOf(0x00, 0xFF.toByte())
        val low = byteArrayOf(0x01, 0x00)
        assertTrue(DuplicateLinkPolicy.compareFingerprints(high, low) < 0)
        assertTrue(DuplicateLinkPolicy.compareFingerprints(low, high) > 0)
        assertEquals(0, DuplicateLinkPolicy.compareFingerprints(high, high.copyOf()))
    }
}
