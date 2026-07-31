package org.racnet.android.ui

import android.content.Intent
import android.net.Uri
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp

/**
 * First-run flow: the SDK-appropriate runtime permissions (with the
 * location rationale where Android still ties BLE scanning to it), then
 * the battery-optimization exemption with per-OEM guidance.
 */
@Composable
fun OnboardingScreen(onReady: () -> Unit) {
    val context = LocalContext.current
    var permissionsGranted by remember { mutableStateOf(Permissions.allGranted(context)) }
    var batteryChecked by remember { mutableStateOf(BatteryOptimization.isIgnoring(context)) }

    val permissionLauncher = rememberLauncherForActivityResult(
        ActivityResultContracts.RequestMultiplePermissions(),
    ) { permissionsGranted = Permissions.allGranted(context) }

    val batteryLauncher = rememberLauncherForActivityResult(
        ActivityResultContracts.StartActivityForResult(),
    ) { batteryChecked = BatteryOptimization.isIgnoring(context) }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(24.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        Text("Racnet setup", style = MaterialTheme.typography.headlineMedium)
        Text(
            "Racnet syncs directly between nearby phones over Bluetooth. " +
                "No servers, no accounts — the mesh is the people carrying it.",
        )

        Card(modifier = Modifier.fillMaxWidth()) {
            Column(
                modifier = Modifier.padding(16.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                Text("Bluetooth", style = MaterialTheme.typography.titleMedium)
                if (Permissions.needsLocationRationale()) {
                    Text(
                        "This Android version requires location permission for " +
                            "Bluetooth scanning. Racnet never reads or stores " +
                            "your location.",
                    )
                }
                if (permissionsGranted) {
                    Text("Granted ✓")
                } else {
                    Button(onClick = {
                        permissionLauncher.launch(Permissions.required().toTypedArray())
                    }) { Text("Grant permissions") }
                }
            }
        }

        Card(modifier = Modifier.fillMaxWidth()) {
            Column(
                modifier = Modifier.padding(16.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                Text("Background operation", style = MaterialTheme.typography.titleMedium)
                Text(
                    "Exempting Racnet from battery optimization keeps the mesh " +
                        "alive while your screen is off.",
                )
                if (batteryChecked) {
                    Text("Exempted ✓")
                } else {
                    Button(onClick = {
                        batteryLauncher.launch(BatteryOptimization.requestIntent(context))
                    }) { Text("Request exemption") }
                }
                BatteryOptimization.oemHelpUrl()?.let { url ->
                    OutlinedButton(onClick = {
                        context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(url)))
                    }) { Text("${android.os.Build.MANUFACTURER} battery settings help") }
                }
            }
        }

        Button(
            onClick = onReady,
            enabled = permissionsGranted,
            modifier = Modifier.fillMaxWidth(),
        ) { Text("Continue") }
    }
}
