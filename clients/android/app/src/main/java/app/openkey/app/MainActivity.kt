package app.openkey.app

import android.content.Intent
import android.provider.Settings
import android.view.inputmethod.InputMethodManager
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import app.openkey.core.OpenKeyEngine
import app.openkey.core.OpenKeyNative

/**
 * OpenKey 主 App（Phase 0/1 骨架：设置 / 启用引导 / 关于）。
 *
 * 导航用简单的状态机（页数少，避免引入 navigation-compose 依赖）。
 */
class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: android.os.Bundle?) {
        super.onCreate(savedInstanceState)
        setContent {
            MaterialTheme {
                OpenKeyApp()
            }
        }
    }
}

private enum class Screen { HOME, SETTINGS, GUIDE, ABOUT }

@Composable
private fun OpenKeyApp() {
    var screen by remember { mutableStateOf(Screen.HOME) }
    when (screen) {
        Screen.HOME -> HomeScreen(
            onOpenSettings = { screen = Screen.SETTINGS },
            onOpenGuide = { screen = Screen.GUIDE },
            onOpenAbout = { screen = Screen.ABOUT },
        )
        Screen.SETTINGS -> SettingsScreen(onBack = { screen = Screen.HOME })
        Screen.GUIDE -> GuideScreen(onBack = { screen = Screen.HOME })
        Screen.ABOUT -> AboutScreen(onBack = { screen = Screen.HOME })
    }
}

@Composable
private fun ScreenScaffold(
    title: String,
    onBack: (() -> Unit)? = null,
    content: @Composable () -> Unit,
) {
    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(text = title, style = MaterialTheme.typography.headlineSmall)
            Spacer(modifier = Modifier.weight(1f))
            if (onBack != null) {
                TextButton(onClick = onBack) { Text("返回") }
            }
        }
        content()
    }
}

@Composable
private fun HomeScreen(
    onOpenSettings: () -> Unit,
    onOpenGuide: () -> Unit,
    onOpenAbout: () -> Unit,
) {
    ScreenScaffold(title = "OpenKey") {
        Text("开源五端中文拼音输入法。你的输入法，你的服务器。")
        MenuButton("输入法设置（方案 / 中英切换）", onOpenSettings)
        MenuButton("启用与切换输入法", onOpenGuide)
        MenuButton("关于", onOpenAbout)
        Text(
            "词库中心 / 云同步 / 密码管理 / 图片表情为后续里程碑 TODO，见 docs/02 第 8 章。",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.outline,
        )
    }
}

@Composable
private fun MenuButton(text: String, onClick: () -> Unit) {
    Button(onClick = onClick, modifier = Modifier.fillMaxWidth()) {
        Text(text)
    }
}

/* ------------------------------------------------------------------ */
/* 设置页                                                              */
/* ------------------------------------------------------------------ */

@Composable
private fun SettingsScreen(onBack: () -> Unit) {
    val context = LocalContext.current
    val engine = remember { OpenKeyEngine.createForContext(context) }
    val session = remember(engine) { engine?.openSession() }
    DisposableEffect(engine, session) {
        onDispose {
            session?.destroy()
            engine?.destroy()
        }
    }

    var schemas by remember { mutableStateOf(emptyList<OpenKeyEngine.SchemaInfo>()) }
    var currentSchema by remember { mutableStateOf<String?>(null) }
    var asciiMode by remember { mutableStateOf(false) }

    LaunchedEffect(engine, session) {
        schemas = engine?.schemas() ?: emptyList()
        currentSchema = session?.currentSchema()
        asciiMode = session?.getOption("ascii_mode") ?: false
    }

    ScreenScaffold(title = "输入法设置", onBack = onBack) {
        if (engine == null || session == null) {
            Text(
                "core 未就绪：libopenkey_ffi.so 未放入 core-binding/src/main/jniLibs，" +
                    "或引擎初始化失败。构建与放置方法见 clients/android/README.md。",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.error,
            )
        }

        Text("输入方案", style = MaterialTheme.typography.titleMedium)
        if (schemas.isEmpty()) {
            Text(
                "（无可用方案：shared 数据目录为空。TODO Phase 1：由 assets 释放 " +
                    "openkey-base CC0 词库的 openkey.schema.yaml，或经词库中心组件下载后 " +
                    "触发 ok_engine_deploy。）",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.outline,
            )
        } else {
            Column(
                modifier = Modifier
                    .heightIn(max = 240.dp)
                    .verticalScroll(rememberScrollState()),
            ) {
                schemas.forEach { schema ->
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .clickable {
                                if (session?.selectSchema(schema.id) == true) {
                                    currentSchema = schema.id
                                }
                            }
                            .padding(vertical = 8.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        RadioButton(
                            selected = currentSchema == schema.id,
                            onClick = {
                                if (session?.selectSchema(schema.id) == true) {
                                    currentSchema = schema.id
                                }
                            },
                        )
                        Text(
                            text = if (schema.name.isNotEmpty()) "${schema.name}（${schema.id}）"
                            else schema.id
                        )
                    }
                }
            }
        }

        // 中英切换：真实生效（core 会话选项 ascii_mode，键盘 Shift 键等价）。
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text("英文模式（中/英切换）", modifier = Modifier.weight(1f))
            Switch(
                checked = asciiMode,
                onCheckedChange = { value ->
                    asciiMode = session?.setOption("ascii_mode", value) ?: false
                },
            )
        }

        // 双拼 / 模糊音：设置项由 core 承载，但依赖词库与 schema 部署，Phase 2 落地。
        Text(
            "双拼方案：TODO（Phase 2：自然码 / 小鹤 / 微软 / 智能 ABC，经词库中心导入 rime schema）。\n" +
                "模糊音：TODO（Phase 2：依赖 rime schema 选项名，随方案部署）。均见 docs/02 5.1。",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.outline,
        )
    }
}

/* ------------------------------------------------------------------ */
/* 启用引导 / 关于                                                      */
/* ------------------------------------------------------------------ */

@Composable
private fun GuideScreen(onBack: () -> Unit) {
    val context = LocalContext.current
    ScreenScaffold(title = "启用与切换输入法", onBack = onBack) {
        Text("1. 在系统设置中启用 OpenKey 输入法")
        Button(onClick = {
            runCatching {
                context.startActivity(
                    Intent(Settings.ACTION_INPUT_METHOD_SETTINGS)
                        .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                )
            }
        }) {
            Text("打开系统输入法设置")
        }
        Text("2. 切换到 OpenKey")
        Button(onClick = {
            context.getSystemService(InputMethodManager::class.java)?.showInputMethodPicker()
        }) {
            Text("选择输入法")
        }
        Text(
            "启用后即可在任意输入框使用。\n" +
                "TODO（Phase 4）：OpenKeyAutofillService（系统自动填充，API 26+）。",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.outline,
        )
    }
}

@Composable
private fun AboutScreen(onBack: () -> Unit) {
    val version = remember {
        if (OpenKeyNative.isLoaded) OpenKeyNative.nativeVersionString() else "core 未加载"
    }
    ScreenScaffold(title = "关于", onBack = onBack) {
        Text("OpenKey 版本：$version")
        Text("开源五端中文拼音输入法（Windows / macOS / Linux / Android / iOS）。")
        Text("许可证：GPL-3.0（客户端）；内置词库与素材为 CC0 / 自制（DR-001）。")
        Text(
            "项目主页与源码：TODO（GitHub 链接）；第三方组件许可见仓库根 THIRD_PARTY_NOTICES.md。",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.outline,
        )
    }
}
