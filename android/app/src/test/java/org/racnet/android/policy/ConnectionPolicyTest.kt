package org.racnet.android.policy

import kotlin.random.Random
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ConnectionPolicyTest {

    private fun policy() = ConnectionPolicy(random = Random(42))

    @Test
    fun `jitter is within the configured window`() {
        val policy = ConnectionPolicy(random = Random(1), jitterMaxMs = 2_000)
        repeat(100) { i ->
            val jitter = policy.shouldConnect("addr-$i", 0)
            assertNotNull(jitter)
            assertTrue(jitter!! in 0..2_000)
        }
    }

    @Test
    fun `a busy address is not connected twice`() {
        val policy = policy()
        assertNotNull(policy.shouldConnect("a", 0))
        assertNull("in-flight attempt owns the address", policy.shouldConnect("a", 0))
        assertNull(policy.shouldConnect("a", 60_000))
    }

    @Test
    fun `clean close frees the address immediately`() {
        val policy = policy()
        assertNotNull(policy.shouldConnect("a", 0))
        policy.closed("a")
        assertNotNull(policy.shouldConnect("a", 1))
    }

    @Test
    fun `failures back off exponentially and cap`() {
        val policy = ConnectionPolicy(
            random = Random(7),
            backoffBaseMs = 5_000,
            backoffCapMs = 300_000,
        )
        var now = 0L
        // First failure: 5 s backoff.
        assertNotNull(policy.shouldConnect("a", now))
        policy.failed("a", now)
        assertNull(policy.shouldConnect("a", now + 4_999))
        now += 5_000
        assertNotNull("backoff elapsed", policy.shouldConnect("a", now))
        // Second failure: 10 s backoff.
        policy.failed("a", now)
        assertNull(policy.shouldConnect("a", now + 9_999))
        now += 10_000
        assertNotNull(policy.shouldConnect("a", now))
        // Many failures: capped at 5 minutes.
        repeat(20) {
            policy.failed("a", now)
            now += 300_000
            assertNotNull("cap reached, never longer", policy.shouldConnect("a", now))
        }
    }

    @Test
    fun `establishment clears the failure history`() {
        val policy = policy()
        var now = 0L
        assertNotNull(policy.shouldConnect("a", now))
        policy.failed("a", now)
        now += 5_000
        assertNotNull(policy.shouldConnect("a", now))
        policy.established("a")
        policy.closed("a")
        // A later failure starts from the base backoff again.
        assertNotNull(policy.shouldConnect("a", now))
        policy.failed("a", now)
        assertNull(policy.shouldConnect("a", now + 4_999))
        assertNotNull(policy.shouldConnect("a", now + 5_000))
    }

    @Test
    fun `addresses are independent`() {
        val policy = policy()
        assertNotNull(policy.shouldConnect("a", 0))
        policy.failed("a", 0)
        assertNotNull("b unaffected by a's backoff", policy.shouldConnect("b", 0))
        assertEquals(false, policy.isBusy("a"))
        assertEquals(true, policy.isBusy("b"))
    }
}
