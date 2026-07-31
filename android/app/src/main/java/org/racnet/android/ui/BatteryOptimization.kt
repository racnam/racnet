package org.racnet.android.ui

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.PowerManager
import android.provider.Settings
import java.util.Locale

/**
 * The OEM battery-killer mitigations the brief requires: the standard
 * exemption check and request, plus per-vendor guidance links
 * (dontkillmyapp.com) for the aggressive OEMs. Whether the exemption
 * suffices per vendor is a measurement question (procedure P4), not a
 * code assumption (ADR-0016).
 */
object BatteryOptimization {

    fun isIgnoring(context: Context): Boolean =
        context.getSystemService(PowerManager::class.java)
            .isIgnoringBatteryOptimizations(context.packageName)

    fun requestIntent(context: Context): Intent =
        Intent(
            Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS,
            Uri.parse("package:${context.packageName}"),
        )

    private val AGGRESSIVE_VENDORS =
        setOf("samsung", "xiaomi", "huawei", "oppo", "oneplus", "vivo", "meizu", "asus")

    /** A dontkillmyapp link for this device's vendor, if it needs one. */
    fun oemHelpUrl(): String? {
        val vendor = Build.MANUFACTURER.lowercase(Locale.ROOT)
        return if (vendor in AGGRESSIVE_VENDORS) "https://dontkillmyapp.com/$vendor" else null
    }
}
