package com.crosbic.phonemedia

import android.app.Activity
import android.app.NotificationManager
import android.content.ActivityNotFoundException
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.Typeface
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.PowerManager
import android.provider.Settings
import android.view.View
import android.widget.Button
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView

private const val LOCAL_NETWORK = "android.permission.ACCESS_LOCAL_NETWORK"

class MainActivity : Activity() {
    private lateinit var pairings: Pairings
    private lateinit var status: TextView
    private lateinit var listenerButton: Button
    private lateinit var networkButton: Button
    private lateinit var batteryButton: Button
    private lateinit var pairingTitle: TextView
    private lateinit var pairingCode: TextView
    private lateinit var paired: TextView
    private lateinit var forgetButton: Button
    private lateinit var log: TextView

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        pairings = Pairings(this)
        val padding = (16 * resources.displayMetrics.density).toInt()

        pairingTitle = TextView(this).apply { textSize = 18f }
        pairingCode = TextView(this).apply {
            textSize = 44f
            typeface = Typeface.create(Typeface.MONOSPACE, Typeface.BOLD)
            setPadding(0, 0, 0, padding)
        }
        status = TextView(this).apply { textSize = 16f }
        listenerButton = Button(this).apply {
            text = "Дать доступ к уведомлениям"
            setOnClickListener { openListenerSettings() }
        }
        networkButton = Button(this).apply {
            text = "Разрешить доступ к локальной сети"
            setOnClickListener { requestPermissions(arrayOf(LOCAL_NETWORK), 1) }
        }
        batteryButton = Button(this).apply {
            text = "Разрешить работу в фоне"
            setOnClickListener {
                startActivity(
                    Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS, Uri.parse("package:$packageName")),
                )
            }
        }
        paired = TextView(this).apply {
            textSize = 14f
            setPadding(0, padding, 0, 0)
        }
        forgetButton = Button(this).apply {
            text = "Забыть все ПК"
            setOnClickListener {
                pairings.forgetAll()
                PcServer.active?.disconnectAll()
                MediaLog.add("все ПК забыты — им нужно сопрячься заново")
                renderPairing()
            }
        }
        log = TextView(this).apply {
            typeface = Typeface.MONOSPACE
            textSize = 12f
            setTextIsSelectable(true)
            setPadding(0, padding, 0, 0)
        }
        val column = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(padding, padding, padding, padding)
            addView(pairingTitle)
            addView(pairingCode)
            addView(status)
            addView(listenerButton)
            addView(networkButton)
            addView(batteryButton)
            addView(paired)
            addView(forgetButton)
            addView(log)
        }
        setContentView(ScrollView(this).apply {
            fitsSystemWindows = true
            addView(column)
        })
    }

    override fun onResume() {
        super.onResume()
        val listenerGranted = getSystemService(NotificationManager::class.java)
            .isNotificationListenerAccessGranted(listenerComponent(this))
        val networkGranted = Build.VERSION.SDK_INT < 37 ||
            checkSelfPermission(LOCAL_NETWORK) == PackageManager.PERMISSION_GRANTED
        val batteryGranted = getSystemService(PowerManager::class.java).isIgnoringBatteryOptimizations(packageName)
        status.text = when {
            !listenerGranted -> "Нужен доступ к уведомлениям: только так Android показывает плееры других приложений."
            !networkGranted -> "Нужен доступ к локальной сети: через неё приложение связывается с ПК."
            !batteryGranted -> "Нужна работа в фоне без ограничений: иначе, когда телефон долго лежит " +
                "с выключенным экраном, Android отключает приложению сеть."
            else -> "Всё готово. Чтобы сопрячь новый ПК, запусти на нём phone-media, пока это приложение открыто."
        }
        listenerButton.visibility = if (listenerGranted) View.GONE else View.VISIBLE
        networkButton.visibility = if (networkGranted) View.GONE else View.VISIBLE
        batteryButton.visibility = if (batteryGranted) View.GONE else View.VISIBLE
        PairingWindow.isOpen = true
        PairingWindow.onChange = ::renderPairing
        renderPairing()
        MediaLog.onChange = ::render
        render()
    }

    override fun onPause() {
        PairingWindow.isOpen = false
        PairingWindow.onChange = null
        MediaLog.onChange = null
        super.onPause()
    }

    private fun render() {
        log.text = MediaLog.lines.joinToString("\n")
    }

    private fun renderPairing() {
        val request = PairingWindow.request
        pairingTitle.text = request?.let { (pcName, _) -> "ПК «$pcName» хочет подключиться. Введи на нём код:" }
        pairingCode.text = request?.second
        pairingTitle.visibility = if (request == null) View.GONE else View.VISIBLE
        pairingCode.visibility = pairingTitle.visibility

        val names = pairings.names()
        paired.text = if (names.isEmpty()) "Сопряжённых ПК нет." else "Сопряжённые ПК: ${names.joinToString(", ")}"
        forgetButton.visibility = if (names.isEmpty()) View.GONE else View.VISIBLE
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
}
