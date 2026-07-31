package org.racnet.android.policy

/**
 * The §9.1.5 duplicate-link rule, as a pure function both peers can
 * evaluate identically: among established links to the same remote
 * fingerprint, keep the one whose initiator is the peer with the
 * lexicographically smaller fingerprint; close the rest. Equal
 * fingerprints mean we are talking to ourselves: close everything.
 */
object DuplicateLinkPolicy {

    /** One established link to the remote, as this side sees it. */
    data class Link(val linkId: ULong, val weInitiated: Boolean)

    /** Bytewise unsigned lexicographic comparison of two fingerprints. */
    fun compareFingerprints(a: ByteArray, b: ByteArray): Int {
        for (i in 0 until minOf(a.size, b.size)) {
            val cmp = (a[i].toInt() and 0xFF).compareTo(b[i].toInt() and 0xFF)
            if (cmp != 0) return cmp
        }
        return a.size.compareTo(b.size)
    }

    /**
     * The link ids to close, given every established link this node
     * holds to one remote fingerprint. Empty when there is no duplicate.
     */
    fun victims(
        localFingerprint: ByteArray,
        remoteFingerprint: ByteArray,
        links: List<Link>,
    ): List<ULong> {
        if (links.size < 2) return emptyList()
        val cmp = compareFingerprints(localFingerprint, remoteFingerprint)
        if (cmp == 0) return links.map { it.linkId }
        // Keep the link initiated by the smaller-fingerprint peer; among
        // several candidates (rare address-rotation races) keep the
        // lowest link id so the choice stays deterministic on this side.
        val keeperInitiatedByUs = cmp < 0
        val keeper = links
            .filter { it.weInitiated == keeperInitiatedByUs }
            .minByOrNull { it.linkId }
            ?: links.minByOrNull { it.linkId }
        return links.map { it.linkId }.filter { it != keeper?.linkId }
    }
}
