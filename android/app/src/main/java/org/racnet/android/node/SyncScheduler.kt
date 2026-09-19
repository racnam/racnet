package org.racnet.android.node

/** Coalesces store changes while a link's snapshot is being reconciled.
 * All calls run under NodeRuntime's mutex. Failed starts stay pending.
 */
internal class SyncScheduler {
    private val pending = mutableSetOf<ULong>()
    private val active = mutableMapOf<ULong, ULong>()

    fun request(linkId: ULong) {
        pending.add(linkId)
    }

    fun completed(linkId: ULong, sessionId: ULong) {
        if (active[linkId] == sessionId) active.remove(linkId)
    }

    fun closed(linkId: ULong) {
        pending.remove(linkId)
        active.remove(linkId)
    }

    /** Call after dispatching an event batch, and on ticks for retries. */
    fun drain(start: (ULong) -> ULong?) {
        for (linkId in pending.toList()) {
            if (linkId in active) continue
            val sessionId = start(linkId) ?: continue
            pending.remove(linkId)
            active[linkId] = sessionId
        }
    }
}
