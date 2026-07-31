package org.racnet.android.metrics

import android.util.Log

/**
 * Measurement records for `docs/MEASUREMENT-PROCEDURES.md`: stable
 * single-line logcat output under one tag so hardware runs are captured
 * with `adb logcat -s RacnetMeas`. The formatter is pure and the fields
 * are ordered, so lines are machine-parseable and diffable.
 */
object Meas {

    const val TAG = "RacnetMeas"

    /** Formats one record: `MEAS event=<name> k=v ...` in given order. */
    fun format(event: String, fields: List<Pair<String, String>>): String =
        buildString {
            append("MEAS event=").append(event)
            for ((key, value) in fields) {
                append(' ').append(key).append('=').append(value)
            }
        }

    fun log(event: String, vararg fields: Pair<String, Any>) {
        Log.i(TAG, format(event, fields.map { (k, v) -> k to v.toString() }))
    }

    /** Throughput in kbit/s from bytes over a duration, one decimal. */
    fun kbps(bytes: Long, durationMs: Long): String {
        if (durationMs <= 0) return "0.0"
        val kbps = bytes * 8.0 / durationMs
        return String.format(java.util.Locale.ROOT, "%.1f", kbps)
    }
}

/**
 * Per-link phase timestamps and byte counters feeding both the
 * diagnostics screen and the `MEAS` lines. All times are
 * `SystemClock.elapsedRealtime()` milliseconds.
 */
class LinkMetrics(val address: String, val initiator: Boolean) {
    var scanFoundAtMs: Long = 0
    var gattConnectedAtMs: Long = 0
    var psmReadAtMs: Long = 0
    var l2capOpenAtMs: Long = 0
    var establishedAtMs: Long = 0
    var syncDoneAtMs: Long = 0

    @Volatile var bytesIn: Long = 0

    @Volatile var bytesOut: Long = 0

    /** The phase deltas as ordered (label, milliseconds) rows. */
    fun phases(): List<Pair<String, Long>> = buildList {
        fun delta(label: String, from: Long, to: Long) {
            if (from > 0 && to >= from) add(label to (to - from))
        }
        delta("scan->gatt", scanFoundAtMs, gattConnectedAtMs)
        delta("gatt->psm", gattConnectedAtMs, psmReadAtMs)
        delta("psm->l2cap", psmReadAtMs, l2capOpenAtMs)
        delta("l2cap->established", l2capOpenAtMs, establishedAtMs)
        delta("established->sync_done", establishedAtMs, syncDoneAtMs)
    }
}
