package org.racnet.android

import android.app.Application
import android.app.NotificationChannel
import android.app.NotificationManager
import org.racnet.android.identity.IdentityStore
import org.racnet.android.node.NodeRuntime

/**
 * Application entry point: creates the notification channel and holds the
 * process-wide singletons (manual service locator; ADR-0016 rejects DI
 * frameworks at this size).
 */
class RacnetApplication : Application() {

    lateinit var identityStore: IdentityStore
        private set

    lateinit var nodeRuntime: NodeRuntime
        private set

    override fun onCreate() {
        super.onCreate()
        identityStore = IdentityStore(this)
        nodeRuntime = NodeRuntime(identityStore.loadOrCreate())

        val manager = getSystemService(NotificationManager::class.java)
        manager.createNotificationChannel(
            NotificationChannel(
                MESH_CHANNEL_ID,
                "Mesh service",
                // Low importance: persistent status, never a sound.
                NotificationManager.IMPORTANCE_LOW,
            ),
        )
    }

    companion object {
        const val MESH_CHANNEL_ID = "mesh"

        fun from(context: android.content.Context): RacnetApplication =
            context.applicationContext as RacnetApplication
    }
}
