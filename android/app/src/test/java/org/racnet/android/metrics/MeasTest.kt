package org.racnet.android.metrics

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Test

class MeasTest {

    @Test
    fun `refresh captures changed metrics without mutating the previous display`() {
        val live = LinkMetrics(address = "test-peer", initiator = true)
        live.l2capOpenAtMs = 1_000
        live.establishedAtMs = 1_100
        live.bytesIn = 100
        val before = listOf(live.snapshot())

        live.bytesIn = 500
        live.bytesOut = 200
        live.syncDoneAtMs = 1_500
        val after = listOf(live.snapshot())

        assertNotEquals(before, after)
        assertEquals(100L, before.single().bytesIn)
        assertEquals(0L, before.single().syncDoneAtMs)
        assertEquals(500L, after.single().bytesIn)
        assertEquals(200L, after.single().bytesOut)
        assertEquals(400L, after.single().phases().toMap()["established->sync_done"])
        assertEquals(after, listOf(live.snapshot()))
    }

    @Test
    fun `records are single stable lines in field order`() {
        val line = Meas.format(
            "sync_done",
            listOf("link" to "3", "bytes_in" to "104230", "dur_ms" to "4180"),
        )
        assertEquals("MEAS event=sync_done link=3 bytes_in=104230 dur_ms=4180", line)
    }

    @Test
    fun `throughput is kilobits per second with one decimal`() {
        // 104 230 bytes in 4 180 ms = 833 840 bits / 4.18 s ≈ 199.5 kbit/s.
        assertEquals("199.5", Meas.kbps(104_230, 4_180))
        assertEquals("0.0", Meas.kbps(1_000, 0))
        assertEquals("0.0", Meas.kbps(0, 1_000))
        assertEquals("8.0", Meas.kbps(1_000, 1_000))
    }

    @Test
    fun `phase deltas appear only when both timestamps exist and order holds`() {
        val metrics = LinkMetrics(address = "AA:BB:CC:DD:EE:FF", initiator = true)
        metrics.scanFoundAtMs = 1_000
        metrics.gattConnectedAtMs = 1_150
        metrics.psmReadAtMs = 1_250
        // l2cap timestamp missing: later phases must not appear.
        metrics.establishedAtMs = 2_000

        val phases = metrics.phases().toMap()
        assertEquals(150L, phases["scan->gatt"])
        assertEquals(100L, phases["gatt->psm"])
        assertEquals(null, phases["psm->l2cap"])
        assertEquals(null, phases["l2cap->established"])
    }
}
