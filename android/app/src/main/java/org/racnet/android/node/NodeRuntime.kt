package org.racnet.android.node

import android.os.SystemClock
import java.util.concurrent.ConcurrentHashMap
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import uniffi.racnet_core.ApiException
import uniffi.racnet_core.CloseCause
import uniffi.racnet_core.EntryView
import uniffi.racnet_core.Event
import uniffi.racnet_core.Identity
import uniffi.racnet_core.LinkIo
import uniffi.racnet_core.LinkOpen
import uniffi.racnet_core.Node
import uniffi.racnet_core.SyncWindow

/**
 * What a link's transport must provide so the runtime can route core
 * output back to the socket that owns the link.
 */
interface LinkTransport {
    /** Write complete frames to the transport, in order. */
    fun writeFrames(frames: List<ByteArray>)

    /**
     * The core closed the link (terminal). The transport tears its
     * connection down; it must not call back into the runtime for this
     * link beyond an idempotent [NodeRuntime.onTransportClosed].
     */
    fun closeFromCore(cause: CloseCause)
}

/**
 * Owns the core [Node] and the discipline around it: the node is a
 * single-threaded state machine, so every FFI call is serialized by one
 * mutex here; the clock is injected from [SystemClock.elapsedRealtimeNanos],
 * which is monotonic and counts deep sleep — the 24-hour link lifetime
 * (spec §4.3) needs both.
 *
 * Frames returned by a call are routed to the [LinkTransport] that owns
 * the named link; events fan out on [events]. On establishment, and after
 * a local entry is created, reconciliation is opened automatically.
 */
class NodeRuntime(identity: Identity) {

    private val node = Node(identity)
    private val lock = Mutex()
    private val transports = ConcurrentHashMap<ULong, LinkTransport>()
    private val establishedLinks = ConcurrentHashMap.newKeySet<ULong>()

    private val _events = MutableSharedFlow<Event>(extraBufferCapacity = 256)

    /** Every core event, in order, for UI and metrics. */
    val events: SharedFlow<Event> = _events

    private val _entryCount = MutableStateFlow(0uL)

    /** Live entry count for the notification and UI. */
    val entryCount: StateFlow<ULong> = _entryCount

    /** Our own 32-byte fingerprint. */
    val fingerprint: ByteArray = node.fingerprint()

    /** Microseconds on the monotonic, sleep-counting clock. */
    fun nowUs(): ULong = (SystemClock.elapsedRealtimeNanos() / 1_000).toULong()

    /**
     * We opened the transport connection (BLE central): Noise initiator.
     * Registers the transport and writes the initiator HELLO through it.
     */
    suspend fun connect(transport: LinkTransport): LinkOpen = lock.withLock {
        val open = node.connect(nowUs())
        transports[open.linkId] = transport
        transport.writeFrames(open.initialFrames)
        open
    }

    /**
     * A remote opened a connection to us (BLE peripheral): Noise
     * responder. Null means the handshake limiter refused it — drop the
     * socket silently (spec §4.5).
     */
    suspend fun accept(remoteAddr: ByteArray, transport: LinkTransport): LinkOpen? =
        lock.withLock {
            val open = node.accept(remoteAddr, nowUs()) ?: return@withLock null
            transports[open.linkId] = transport
            open
        }

    /** Raw bytes from a link's socket, any segmentation. */
    suspend fun onBytes(linkId: ULong, bytes: ByteArray) = lock.withLock {
        dispatch(linkId, node.onBytes(linkId, bytes, nowUs()))
    }

    /** The link's socket died. Idempotent. */
    suspend fun onTransportClosed(linkId: ULong) = lock.withLock {
        dispatchEvents(node.onTransportClosed(linkId, nowUs()))
    }

    /** Signs and stores a local entry, then pushes it to every peer. */
    suspend fun createEntry(kind: ULong, payload: ByteArray): EntryView = lock.withLock {
        val view = node.createEntry(kind, payload, System.currentTimeMillis().toULong())
        _entryCount.value = node.entryCount()
        establishedLinks.forEach { linkId -> startSyncLocked(linkId) }
        view
    }

    /** The stored entries, newest window first left to the caller. */
    suspend fun entries(): List<EntryView> = lock.withLock {
        node.entries(SyncWindow(sinceMs = 0uL, untilMs = ULong.MAX_VALUE))
    }

    /** Established-link fingerprint, if the link is live and established. */
    suspend fun remoteFingerprint(linkId: ULong): ByteArray? = lock.withLock {
        node.linkStatus(linkId)?.remoteFingerprint
    }

    /**
     * Runs the 1-second tick loop enforcing the 24-hour lifetime cap and
     * the half-open handshake timeout, for as long as `scope` lives.
     */
    fun startTicking(scope: CoroutineScope): Job = scope.launch {
        while (true) {
            delay(1_000)
            lock.withLock { dispatchEvents(node.tick(nowUs())) }
        }
    }

    /** Routes one call's output: frames to the owning transport, then events. */
    private fun dispatch(linkId: ULong, io: LinkIo) {
        if (io.frames.isNotEmpty()) {
            transports[linkId]?.writeFrames(io.frames)
        }
        dispatchEvents(io.events)
    }

    private fun dispatchEvents(events: List<Event>) {
        for (event in events) {
            when (event) {
                is Event.Established -> {
                    establishedLinks.add(event.linkId)
                    startSyncLocked(event.linkId)
                }
                is Event.Closed -> {
                    establishedLinks.remove(event.linkId)
                    transports.remove(event.linkId)?.closeFromCore(event.cause)
                }
                is Event.EntryStored -> _entryCount.value = node.entryCount()
                else -> {}
            }
            _events.tryEmit(event)
        }
    }

    /**
     * Opens a full-window reconciliation session. Callers hold [lock].
     * Refusals (session cap, closing race) are not fatal: the next
     * establishment or entry retries.
     */
    private fun startSyncLocked(linkId: ULong) {
        val start = try {
            node.startSync(linkId, SyncWindow(sinceMs = 0uL, untilMs = ULong.MAX_VALUE))
        } catch (e: ApiException) {
            return
        }
        dispatch(linkId, start.io)
    }
}
