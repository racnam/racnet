package org.racnet.android.mesh

import android.os.SystemClock
import java.util.concurrent.ConcurrentHashMap
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import org.racnet.android.metrics.Meas
import org.racnet.android.policy.DuplicateLinkPolicy

/** One established peer, as shown in the UI and the notification. */
data class PeerInfo(
    val linkId: ULong,
    val fingerprintHex: String,
    val initiator: Boolean,
    val establishedAtMs: Long,
)

/**
 * The live connections: link id → connection, in-flight/open addresses
 * for the central's dedup, established peers by fingerprint for the
 * §9.1.5 duplicate rule, and the flows the notification and UI observe.
 */
class ConnectionRegistry(private val localFingerprint: ByteArray) {

    private val byLinkId = ConcurrentHashMap<ULong, LinkConnection>()
    private val establishedFingerprints = ConcurrentHashMap<ULong, ByteArray>()

    private val _peers = MutableStateFlow<List<PeerInfo>>(emptyList())

    /** Established peers, for the status screen and notification. */
    val peers: StateFlow<List<PeerInfo>> = _peers

    fun register(connection: LinkConnection) {
        byLinkId[connection.linkId] = connection
    }

    fun remove(connection: LinkConnection) {
        byLinkId.remove(connection.linkId)
        establishedFingerprints.remove(connection.linkId)
        publish()
    }

    /** Whether any live connection uses this BLE address. */
    fun hasAddress(address: String): Boolean =
        byLinkId.values.any { it.address == address }

    fun connection(linkId: ULong): LinkConnection? = byLinkId[linkId]

    /** Every live connection, for the diagnostics screen. */
    fun connectionsSnapshot(): List<LinkConnection> = byLinkId.values.toList()

    /**
     * A link established: record it, mark its metrics, and apply the
     * duplicate rule — victims are shut down, which silently closes them
     * on both ends (§9.1.5).
     */
    fun onEstablished(linkId: ULong, remoteFingerprint: ByteArray) {
        val connection = byLinkId[linkId] ?: return
        connection.metrics.establishedAtMs = SystemClock.elapsedRealtime()
        establishedFingerprints[linkId] = remoteFingerprint
        Meas.log(
            "established",
            "link" to linkId,
            "role" to if (connection.initiator) "initiator" else "responder",
            "l2cap_to_established_ms" to
                (connection.metrics.establishedAtMs - connection.metrics.l2capOpenAtMs),
        )

        val sameRemote = establishedFingerprints.filter { (_, fp) ->
            fp.contentEquals(remoteFingerprint)
        }
        val victims = DuplicateLinkPolicy.victims(
            localFingerprint,
            remoteFingerprint,
            sameRemote.map { (id, _) ->
                DuplicateLinkPolicy.Link(id, byLinkId[id]?.initiator == true)
            },
        )
        for (victim in victims) {
            Meas.log("duplicate_link_closed", "link" to victim)
            byLinkId[victim]?.shutdown(reportToCore = true)
        }
        publish()
    }

    /** Tears every connection down (service stop). */
    fun shutdownAll() {
        byLinkId.values.toList().forEach { it.shutdown(reportToCore = true) }
    }

    private fun publish() {
        _peers.value = establishedFingerprints.mapNotNull { (linkId, fp) ->
            val connection = byLinkId[linkId] ?: return@mapNotNull null
            PeerInfo(
                linkId = linkId,
                fingerprintHex = fp.joinToString("") { "%02x".format(it) },
                initiator = connection.initiator,
                establishedAtMs = connection.metrics.establishedAtMs,
            )
        }.sortedBy { it.establishedAtMs }
    }
}
