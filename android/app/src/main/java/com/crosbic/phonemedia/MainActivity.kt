package com.crosbic.phonemedia

import android.app.NotificationManager
import android.content.ActivityNotFoundException
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.PowerManager
import android.provider.Settings
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue

private const val LOCAL_NETWORK = "android.permission.ACCESS_LOCAL_NETWORK"

class MainActivity : ComponentActivity() {
    private val pairings by lazy { Pairings(this) }
    private var setup by mutableStateOf(Setup(listener = false, network = false, battery = false))
    private val requestNetwork = registerForActivityResult(ActivityResultContracts.RequestPermission()) { refresh() }

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        setContent {
            PhoneMediaTheme {
                PhoneMediaApp(
                    setup = setup,
                    actions = SetupActions(
                        grantListener = ::openListenerSettings,
                        grantNetwork = { requestNetwork.launch(LOCAL_NETWORK) },
                        grantBattery = ::openBatterySettings,
                    ),
                    onForget = ::forget,
                )
            }
        }
    }

    override fun onResume() {
        super.onResume()
        PairingWindow.isOpen = true
        refresh()
        pairings.publish()
    }

    override fun onPause() {
        PairingWindow.isOpen = false
        super.onPause()
    }

    private fun refresh() {
        setup = Setup(
            listener = getSystemService(NotificationManager::class.java)
                .isNotificationListenerAccessGranted(listenerComponent(this)),
            network = Build.VERSION.SDK_INT < 37 ||
                checkSelfPermission(LOCAL_NETWORK) == PackageManager.PERMISSION_GRANTED,
            battery = getSystemService(PowerManager::class.java).isIgnoringBatteryOptimizations(packageName),
        )
    }

    private fun forget(pc: Pc) {
        pairings.forget(pc.id)
        PcServer.active?.disconnect(pc.id)
        MediaLog.add("ПК «${pc.name}» забыт: чтобы подключить его снова, нужно новое сопряжение")
    }

    private fun openListenerSettings() {
        val ownToggle = Intent(Settings.ACTION_NOTIFICATION_LISTENER_DETAIL_SETTINGS)
            .putExtra(Settings.EXTRA_NOTIFICATION_LISTENER_COMPONENT_NAME, listenerComponent(this).flattenToString())
        try {
            startActivity(ownToggle)
        } catch (_: ActivityNotFoundException) {
            startActivity(Intent(Settings.ACTION_NOTIFICATION_LISTENER_SETTINGS))
        }
    }

    private fun openBatterySettings() {
        startActivity(Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS, Uri.parse("package:$packageName")))
    }
}
