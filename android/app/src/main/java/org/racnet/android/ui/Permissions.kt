package org.racnet.android.ui

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import android.os.Build

/**
 * The runtime-permission set, split by SDK generation (ADR-0016):
 * API 31+ uses the granular Bluetooth permissions; API 29-30 needs fine
 * location for BLE scanning (the legacy Bluetooth permissions are
 * manifest-only there).
 */
object Permissions {

    fun required(): List<String> =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            buildList {
                add(Manifest.permission.BLUETOOTH_SCAN)
                add(Manifest.permission.BLUETOOTH_CONNECT)
                add(Manifest.permission.BLUETOOTH_ADVERTISE)
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                    add(Manifest.permission.POST_NOTIFICATIONS)
                }
            }
        } else {
            listOf(Manifest.permission.ACCESS_FINE_LOCATION)
        }

    fun allGranted(context: Context): Boolean =
        required().all { permission ->
            context.checkSelfPermission(permission) == PackageManager.PERMISSION_GRANTED
        }

    /** Whether the location rationale applies (API 29-30 scan rule). */
    fun needsLocationRationale(): Boolean =
        Build.VERSION.SDK_INT < Build.VERSION_CODES.S
}
