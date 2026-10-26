package com.crosbic.phonemedia

import android.graphics.Bitmap
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue

data class NowPlaying(
    val app: String,
    val title: String,
    val artist: String,
    val playing: Boolean,
    val art: Bitmap?,
)

data class PairingRequest(val pcName: String, val code: String)

data class Pc(val id: String, val name: String)

object AppState {
    var nowPlaying by mutableStateOf<NowPlaying?>(null)
    var connectedPcs by mutableStateOf<List<Pc>>(emptyList())
    var pairedPcs by mutableStateOf<List<Pc>>(emptyList())
    var pairingRequest by mutableStateOf<PairingRequest?>(null)
    var update by mutableStateOf<UpdateState>(UpdateState.Idle)
    val log = mutableStateListOf<String>()
}
