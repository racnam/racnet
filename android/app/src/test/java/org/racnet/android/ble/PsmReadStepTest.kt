package org.racnet.android.ble

import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Test

class PsmReadStepTest {
    @Test
    fun `permission revoked in an asynchronous callback ends the read`() = runTest {
        val result = CompletableDeferred<Int?>()
        requestPsmReadStep(result) { throw SecurityException("permission revoked") }

        assertNull(result.await())
        assertFalse(result.complete(128))
    }

    @Test
    fun `a rejected operation ends the read without waiting for timeout`() = runTest {
        val result = CompletableDeferred<Int?>()
        requestPsmReadStep(result) { false }
        assertNull(result.await())
    }

    @Test
    fun `an accepted operation waits for its result callback`() = runTest {
        val result = CompletableDeferred<Int?>()
        requestPsmReadStep(result) { true }
        assertFalse(result.isCompleted)

        result.complete(128)
        assertEquals(128, result.await())
    }

    @Test
    fun `late callbacks after completion or cancellation do not start another operation`() {
        val completed = CompletableDeferred<Int?>().apply { complete(128) }
        val cancelled = CompletableDeferred<Int?>().apply { cancel() }
        var requests = 0

        requestPsmReadStep(completed) { requests++; true }
        requestPsmReadStep(cancelled) { requests++; true }

        assertEquals(0, requests)
    }
}
