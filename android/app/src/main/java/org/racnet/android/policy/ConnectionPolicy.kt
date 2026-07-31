package org.racnet.android.policy

import kotlin.random.Random

/**
 * Pure connection-attempt state machine for the central role: decides,
 * per discovered address, whether to connect and after what jitter
 * (§9.1.5), and applies per-address exponential backoff after failures.
 * No clocks, no radios — time is injected — so it is fully unit-testable.
 */
class ConnectionPolicy(
    private val random: Random = Random.Default,
    private val jitterMaxMs: Long = 2_000,
    private val backoffBaseMs: Long = 5_000,
    private val backoffCapMs: Long = 300_000,
) {

    private sealed interface State {
        /** A connect attempt or live connection owns this address. */
        data object Busy : State

        /** Failed recently; ignore until the deadline. */
        data class Backoff(val untilMs: Long, val failures: Int) : State
    }

    private val states = HashMap<String, State>()

    /**
     * A scan result arrived for `address`. Returns the jitter delay in
     * milliseconds to wait before connecting, or null to ignore (already
     * busy, or backing off). The address is marked busy immediately so
     * repeated scan callbacks during the jitter window do not stack
     * attempts.
     */
    fun shouldConnect(address: String, nowMs: Long): Long? {
        val prior = states[address]
        when (prior) {
            is State.Busy -> return null
            is State.Backoff ->
                if (nowMs < prior.untilMs) return null
            null -> {}
        }
        // Preserve the failure count across the busy attempt; jitter is
        // uniform in [0, jitterMaxMs].
        pendingFailures[address] = (prior as? State.Backoff)?.failures ?: 0
        states[address] = State.Busy
        return random.nextLong(jitterMaxMs + 1)
    }

    private val pendingFailures = HashMap<String, Int>()

    /** The attempt (or the connection it produced) ended in failure. */
    fun failed(address: String, nowMs: Long) {
        val failures = (pendingFailures.remove(address) ?: 0) + 1
        val shift = (failures - 1).coerceAtMost(10)
        val backoff = (backoffBaseMs shl shift).coerceAtMost(backoffCapMs)
        states[address] = State.Backoff(nowMs + backoff, failures)
    }

    /** The connection closed cleanly; the address may be retried at once. */
    fun closed(address: String) {
        pendingFailures.remove(address)
        states.remove(address)
    }

    /** The connection established; clear the failure history. */
    fun established(address: String) {
        pendingFailures[address] = 0
    }

    /** Whether the address currently has an attempt or connection. */
    fun isBusy(address: String): Boolean = states[address] is State.Busy
}
