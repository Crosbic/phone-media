package com.crosbic.phonemedia

import android.graphics.Bitmap
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.requiredSize
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.Article
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowRight
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Computer
import androidx.compose.material.icons.filled.MusicNote
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

data class Setup(val listener: Boolean, val network: Boolean, val battery: Boolean) {
    val done get() = listener && network && battery
}

class SetupActions(
    val grantListener: () -> Unit,
    val grantNetwork: () -> Unit,
    val grantBattery: () -> Unit,
)

private val LightColors = lightColorScheme(
    primary = Color(0xFF137A7F),
    onPrimary = Color.White,
    primaryContainer = Color(0xFFB9ECEA),
    onPrimaryContainer = Color(0xFF00363A),
    secondary = Color(0xFF4A6364),
    onSecondary = Color.White,
    secondaryContainer = Color(0xFFCCE8E8),
    onSecondaryContainer = Color(0xFF051F20),
    background = Color(0xFFF4FBFA),
    onBackground = Color(0xFF161D1D),
    surface = Color(0xFFF4FBFA),
    onSurface = Color(0xFF161D1D),
    surfaceVariant = Color(0xFFDAE4E4),
    onSurfaceVariant = Color(0xFF3F4948),
    surfaceContainerLowest = Color.White,
    surfaceContainerLow = Color(0xFFEFF5F4),
    surfaceContainer = Color(0xFFE9EFEE),
    surfaceContainerHigh = Color(0xFFE3E9E9),
    surfaceContainerHighest = Color(0xFFDDE4E3),
    outline = Color(0xFF6F7979),
    outlineVariant = Color(0xFFBEC9C8),
)

private val DarkColors = darkColorScheme(
    primary = Color(0xFF86CECB),
    onPrimary = Color(0xFF003739),
    primaryContainer = Color(0xFF005052),
    onPrimaryContainer = Color(0xFFA5F0EC),
    secondary = Color(0xFFB0CCCB),
    onSecondary = Color(0xFF1B3435),
    secondaryContainer = Color(0xFF324B4B),
    onSecondaryContainer = Color(0xFFCCE8E7),
    background = Color(0xFF0E1514),
    onBackground = Color(0xFFDDE4E3),
    surface = Color(0xFF0E1514),
    onSurface = Color(0xFFDDE4E3),
    surfaceVariant = Color(0xFF3F4948),
    onSurfaceVariant = Color(0xFFBEC9C8),
    surfaceContainerLowest = Color(0xFF090F0F),
    surfaceContainerLow = Color(0xFF161D1D),
    surfaceContainer = Color(0xFF1A2121),
    surfaceContainerHigh = Color(0xFF252B2B),
    surfaceContainerHighest = Color(0xFF303636),
    outline = Color(0xFF889392),
    outlineVariant = Color(0xFF3F4948),
)

private val BrandGradient = Brush.linearGradient(
    0f to Color(0xFF86CECB),
    0.6f to Color(0xFF137A7F),
    1f to Color(0xFF0F6569),
)

@Composable
fun PhoneMediaTheme(content: @Composable () -> Unit) {
    MaterialTheme(colorScheme = if (isSystemInDarkTheme()) DarkColors else LightColors, content = content)
}

@Composable
fun PhoneMediaApp(setup: Setup, actions: SetupActions, onForget: (Pc) -> Unit) {
    var showLog by rememberSaveable { mutableStateOf(false) }
    val request = AppState.pairingRequest
    when {
        request != null -> PairingScreen(request)
        !setup.done -> SetupScreen(setup, actions)
        showLog -> LogScreen(onBack = { showLog = false })
        else -> MainScreen(onForget = onForget, onOpenLog = { showLog = true })
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun Screen(
    title: String,
    brand: Boolean = false,
    onBack: (() -> Unit)? = null,
    content: @Composable ColumnScope.() -> Unit,
) {
    Scaffold(
        topBar = {
            TopAppBar(
                title = {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        if (brand) {
                            AppMark(30.dp)
                            Spacer(Modifier.width(12.dp))
                        }
                        Text(title)
                    }
                },
                navigationIcon = {
                    if (onBack != null) {
                        IconButton(onClick = onBack) {
                            Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Назад")
                        }
                    }
                },
            )
        },
    ) { padding ->
        Column(
            modifier = Modifier
                .padding(padding)
                .fillMaxSize()
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 16.dp, vertical = 8.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
            content = content,
        )
    }
}

@Composable
private fun AppMark(size: Dp) {
    Box(
        modifier = Modifier
            .size(size)
            .clip(RoundedCornerShape(size * 0.24f))
            .background(BrandGradient),
        contentAlignment = Alignment.Center,
    ) {
        Image(
            painter = painterResource(R.drawable.ic_launcher_foreground),
            contentDescription = null,
            modifier = Modifier.requiredSize(size * 1.42f),
        )
    }
}

@Composable
private fun MainScreen(onForget: (Pc) -> Unit, onOpenLog: () -> Unit) {
    var forgetting by remember { mutableStateOf<Pc?>(null) }
    val connected = AppState.connectedPcs
    Screen(title = "Phone Media", brand = true) {
        ConnectionCard(connected)
        NowPlayingCard(AppState.nowPlaying, connected = connected.isNotEmpty())
        Text(
            text = "Компьютеры",
            style = MaterialTheme.typography.titleSmall,
            color = MaterialTheme.colorScheme.primary,
            modifier = Modifier.padding(start = 4.dp, top = 8.dp),
        )
        val paired = AppState.pairedPcs
        if (paired.isEmpty()) {
            Text(
                text = "Компьютеров пока нет. Запусти Phone Media на компьютере в той же сети: " +
                    "пока это приложение открыто, его можно сопрячь.",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(horizontal = 4.dp),
            )
        }
        paired.forEach { pc ->
            PcCard(pc, online = connected.any { it.id == pc.id }, onForget = { forgetting = pc })
        }
        Spacer(Modifier.height(4.dp))
        Card(onClick = onOpenLog, modifier = Modifier.fillMaxWidth(), colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainer)) {
            Row(
                modifier = Modifier.padding(horizontal = 16.dp, vertical = 14.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(16.dp),
            ) {
                Icon(Icons.AutoMirrored.Filled.Article, contentDescription = null, tint = MaterialTheme.colorScheme.onSurfaceVariant)
                Text("Журнал", style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f))
                Icon(Icons.AutoMirrored.Filled.KeyboardArrowRight, contentDescription = null, tint = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }
    }
    forgetting?.let { pc ->
        AlertDialog(
            onDismissRequest = { forgetting = null },
            title = { Text("Забыть «${pc.name}»?") },
            text = { Text("Компьютер отключится. Чтобы подключить его снова, понадобится новое сопряжение.") },
            confirmButton = {
                TextButton(onClick = {
                    onForget(pc)
                    forgetting = null
                }) { Text("Забыть") }
            },
            dismissButton = { TextButton(onClick = { forgetting = null }) { Text("Отмена") } },
        )
    }
}

@Composable
private fun ConnectionCard(connected: List<Pc>) {
    val online = connected.isNotEmpty()
    Card(modifier = Modifier.fillMaxWidth(), colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainer)) {
        Row(
            modifier = Modifier.padding(16.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            Box(
                modifier = Modifier
                    .size(44.dp)
                    .clip(CircleShape)
                    .background(if (online) MaterialTheme.colorScheme.primaryContainer else MaterialTheme.colorScheme.surfaceVariant),
                contentAlignment = Alignment.Center,
            ) {
                Icon(
                    Icons.Filled.Computer,
                    contentDescription = null,
                    tint = if (online) MaterialTheme.colorScheme.onPrimaryContainer else MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            Column {
                Text(
                    text = if (online) "Подключено к компьютеру" else "Компьютер не подключён",
                    style = MaterialTheme.typography.titleMedium,
                )
                Text(
                    text = if (online) connected.joinToString { it.name } else "Запусти Phone Media на компьютере в той же сети",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}

@Composable
private fun NowPlayingCard(now: NowPlaying?, connected: Boolean) {
    Card(modifier = Modifier.fillMaxWidth(), colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainer)) {
        Row(
            modifier = Modifier.padding(16.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            Cover(now?.art)
            Column(Modifier.weight(1f)) {
                if (now == null) {
                    Text("Ничего не играет", style = MaterialTheme.typography.titleMedium)
                    Text(
                        text = "Включи музыку в любом плеере",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                } else {
                    Text(
                        text = now.title.ifEmpty { "Без названия" },
                        style = MaterialTheme.typography.titleMedium,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                    if (now.artist.isNotEmpty()) {
                        Text(now.artist, style = MaterialTheme.typography.bodyMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    }
                    Text(now.app, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    Spacer(Modifier.height(8.dp))
                    StatusChip(
                        text = when {
                            !connected -> "Не видно на компьютере"
                            now.playing -> "Играет · видно на компьютере"
                            else -> "Пауза · видно на компьютере"
                        },
                        active = connected,
                    )
                }
            }
        }
    }
}

@Composable
private fun Cover(art: Bitmap?) {
    Box(
        modifier = Modifier
            .size(72.dp)
            .clip(RoundedCornerShape(12.dp))
            .background(MaterialTheme.colorScheme.primaryContainer),
        contentAlignment = Alignment.Center,
    ) {
        if (art != null) {
            val image = remember(art) { art.asImageBitmap() }
            Image(image, contentDescription = null, contentScale = ContentScale.Crop, modifier = Modifier.fillMaxSize())
        } else {
            Icon(Icons.Filled.MusicNote, contentDescription = null, tint = MaterialTheme.colorScheme.onPrimaryContainer)
        }
    }
}

@Composable
private fun StatusChip(text: String, active: Boolean) {
    Surface(
        shape = RoundedCornerShape(8.dp),
        color = if (active) MaterialTheme.colorScheme.secondaryContainer else MaterialTheme.colorScheme.surfaceVariant,
    ) {
        Text(
            text = text,
            style = MaterialTheme.typography.labelMedium,
            color = if (active) MaterialTheme.colorScheme.onSecondaryContainer else MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(horizontal = 10.dp, vertical = 4.dp),
        )
    }
}

@Composable
private fun PcCard(pc: Pc, online: Boolean, onForget: () -> Unit) {
    Card(modifier = Modifier.fillMaxWidth(), colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainer)) {
        Row(
            modifier = Modifier.padding(start = 16.dp, end = 8.dp, top = 8.dp, bottom = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Column(Modifier.weight(1f)) {
                Text(pc.name, style = MaterialTheme.typography.bodyLarge)
                Text(
                    text = if (online) "Подключён" else "Не в сети",
                    style = MaterialTheme.typography.bodySmall,
                    color = if (online) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            TextButton(onClick = onForget) { Text("Забыть") }
        }
    }
}

private class Step(val title: String, val description: String, val done: Boolean, val grant: () -> Unit)

@Composable
private fun SetupScreen(setup: Setup, actions: SetupActions) {
    val steps = listOf(
        Step("Доступ к уведомлениям", "Так Android показывает приложению плееры", setup.listener, actions.grantListener),
        Step("Локальная сеть", "Чтобы компьютер нашёл телефон", setup.network, actions.grantNetwork),
        Step("Работа в фоне", "Чтобы связь не рвалась, когда телефон спит", setup.battery, actions.grantBattery),
    )
    val current = steps.indexOfFirst { !it.done }
    Screen(title = "Настройка", brand = true) {
        Text(
            text = "Три разрешения — и музыка с телефона появится в Windows.",
            style = MaterialTheme.typography.bodyLarge,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(horizontal = 4.dp),
        )
        steps.forEachIndexed { index, step -> StepCard(index + 1, step, current = index == current) }
        if (current == 0) {
            Text(
                text = "Если переключатель серый: Настройки → Приложения → Phone Media → ⋮ → " +
                    "Разрешить ограниченные настройки.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(horizontal = 4.dp),
            )
        }
    }
}

@Composable
private fun StepCard(number: Int, step: Step, current: Boolean) {
    val container = if (current) MaterialTheme.colorScheme.surfaceContainerHigh else MaterialTheme.colorScheme.surfaceContainer
    Card(modifier = Modifier.fillMaxWidth(), colors = CardDefaults.cardColors(containerColor = container)) {
        Column(Modifier.fillMaxWidth().padding(16.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(16.dp)) {
                Box(
                    modifier = Modifier
                        .size(32.dp)
                        .clip(CircleShape)
                        .background(
                            when {
                                step.done -> MaterialTheme.colorScheme.primary
                                current -> MaterialTheme.colorScheme.primaryContainer
                                else -> MaterialTheme.colorScheme.surfaceVariant
                            },
                        ),
                    contentAlignment = Alignment.Center,
                ) {
                    if (step.done) {
                        Icon(Icons.Filled.Check, contentDescription = "Готово", tint = MaterialTheme.colorScheme.onPrimary, modifier = Modifier.size(18.dp))
                    } else {
                        Text(
                            text = "$number",
                            style = MaterialTheme.typography.labelLarge,
                            color = if (current) MaterialTheme.colorScheme.onPrimaryContainer else MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
                Column(Modifier.weight(1f)) {
                    Text(
                        text = step.title,
                        style = MaterialTheme.typography.titleMedium,
                        color = if (step.done || current) MaterialTheme.colorScheme.onSurface else MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    Text(step.description, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
            if (current) {
                Spacer(Modifier.height(12.dp))
                Button(onClick = step.grant, modifier = Modifier.align(Alignment.End)) { Text("Разрешить") }
            }
        }
    }
}

@Composable
private fun PairingScreen(request: PairingRequest) {
    Screen(title = "Новый компьютер") {
        Card(modifier = Modifier.fillMaxWidth(), colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.primaryContainer)) {
            Column(
                modifier = Modifier.fillMaxWidth().padding(horizontal = 24.dp, vertical = 32.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                Icon(
                    Icons.Filled.Computer,
                    contentDescription = null,
                    tint = MaterialTheme.colorScheme.onPrimaryContainer,
                    modifier = Modifier.size(40.dp),
                )
                Text(request.pcName, style = MaterialTheme.typography.titleLarge, color = MaterialTheme.colorScheme.onPrimaryContainer)
                Text("хочет подключиться", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onPrimaryContainer)
                Spacer(Modifier.height(12.dp))
                Text(
                    text = request.code,
                    style = MaterialTheme.typography.displayMedium.copy(fontFamily = FontFamily.Monospace, letterSpacing = 4.sp),
                    color = MaterialTheme.colorScheme.onPrimaryContainer,
                )
                Spacer(Modifier.height(4.dp))
                Text(
                    text = "Введи этот код в окне на компьютере",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onPrimaryContainer,
                    textAlign = TextAlign.Center,
                )
            }
        }
        Text(
            text = "Код действует, пока открыт этот экран.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(horizontal = 4.dp),
        )
    }
}

@Composable
private fun LogScreen(onBack: () -> Unit) {
    BackHandler(onBack = onBack)
    Screen(title = "Журнал", onBack = onBack) {
        SelectionContainer {
            Text(
                text = AppState.log.joinToString("\n").ifEmpty { "Пока пусто" },
                style = MaterialTheme.typography.bodySmall.copy(fontFamily = FontFamily.Monospace),
            )
        }
    }
}
