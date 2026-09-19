package org.racnet.android.node

import org.junit.Assert.assertEquals
import org.junit.Test

class SyncSchedulerTest {
    @Test fun changesDuringSnapshotAreCoalescedIntoOneFollowUp() {
        val scheduler = SyncScheduler()
        var starts = 0
        val start: (ULong) -> ULong? = { (++starts).toULong() }
        scheduler.request(7uL)
        scheduler.drain(start)
        repeat(100) { scheduler.request(7uL); scheduler.drain(start) }
        assertEquals(1, starts)
        scheduler.completed(7uL, 999uL)
        scheduler.drain(start)
        assertEquals(1, starts)
        scheduler.completed(7uL, 1uL)
        scheduler.drain(start)
        assertEquals(2, starts)
        scheduler.completed(7uL, 2uL)
        scheduler.drain(start)
        assertEquals(2, starts)
    }

    @Test fun failedStartRetriesWithoutAnotherEntryAndCloseDiscardsPendingWork() {
        val scheduler = SyncScheduler()
        var attempts = 0
        scheduler.request(1uL)
        scheduler.drain { attempts++; null }
        scheduler.drain { attempts++; 2uL }
        assertEquals(2, attempts)
        scheduler.request(1uL)
        scheduler.closed(1uL)
        scheduler.drain { attempts++; 3uL }
        assertEquals(2, attempts)
    }

    @Test fun busyLinkDoesNotBlockOtherLinks() {
        val scheduler = SyncScheduler()
        val started = mutableListOf<ULong>()
        val start: (ULong) -> ULong? = { started.add(it); it }
        scheduler.request(1uL)
        scheduler.drain(start)
        scheduler.request(1uL)
        scheduler.request(2uL)
        scheduler.drain(start)
        assertEquals(listOf(1uL, 2uL), started)
    }
}
