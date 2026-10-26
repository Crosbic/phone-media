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
    private val main = Handler(Looper.getMainLooper())

    fun post(line: String) {
        main.post { add(line) }
    }

    fun add(line: String) {
        Log.i(TAG, line)
        AppState.log.add(0, "${LocalTime.now().format(timeFormat)}  $line")
        if (AppState.log.size > LIMIT) AppState.log.removeAt(AppState.log.lastIndex)
    }
}
