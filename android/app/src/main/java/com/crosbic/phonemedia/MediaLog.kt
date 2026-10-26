package com.crosbic.phonemedia

import android.os.Handler
import android.os.Looper
import android.util.Log
import java.time.LocalTime
import java.time.format.DateTimeFormatter

object MediaLog {
    private const val TAG = "PhoneMedia"
    private const val LIMIT = 200
    private val timeFormat = DateTimeFormatter.ofPattern("HH:mm:ss")

    val lines = ArrayDeque<String>()
    var onChange: (() -> Unit)? = null

    private val main = Handler(Looper.getMainLooper())

    fun post(line: String) {
        main.post { add(line) }
    }

    fun add(line: String) {
        Log.i(TAG, line)
        lines.addFirst("${LocalTime.now().format(timeFormat)}  $line")
        if (lines.size > LIMIT) lines.removeLast()
        onChange?.invoke()
    }
}
