import SwiftUI
import OpenKeyCore

/// 设置页（macOS 13+）：方案选择、双拼、模糊音、中英切换。
///
/// 写入的偏好经 core 数据层持久化，输入法进程在会话创建时读取生效
/// （Phase 2 计划做设置变更热更新，见 README TODO）。
struct SettingsView: View {
    @StateObject private var model = SettingsModel()

    var body: some View {
        Form {
            if let error = model.engineError {
                Section {
                    Label(error, systemImage: "exclamationmark.triangle")
                        .foregroundStyle(.red)
                        .textSelection(.enabled)
                } header: {
                    Text("引擎未就绪")
                } footer: {
                    Text("请先按 clients/macos/README.md 准备 core 库与 rime 数据目录。")
                }
            }

            Section {
                Picker("输入方案", selection: $model.schemaId) {
                    ForEach(model.schemas) { schema in
                        Text(schema.name).tag(schema.id)
                    }
                }
                .disabled(model.schemas.isEmpty)
                if model.schemas.isEmpty && model.engineError == nil {
                    Text("尚未部署任何方案：请将 shared/schemas 部署到 ~/Library/Application Support/OpenKey/shared（见 README）。")
                        .font(.callout)
                        .foregroundStyle(.secondary)
                }
            } header: {
                Text("输入方案")
            }

            Section {
                Picker("双拼方案", selection: $model.doublePinyin) {
                    ForEach(DoublePinyinScheme.allCases, id: \.self) { scheme in
                        Text(scheme.title).tag(scheme)
                    }
                }
                .pickerStyle(.menu)
            } header: {
                Text("双拼")
            } footer: {
                Text("选择非“关闭”时优先使用对应双拼方案（shared/schemas/openkey_double_*.schema.yaml）。")
            }

            Section {
                Toggle("模糊音", isOn: $model.fuzzyPinyin)
            } header: {
                Text("模糊音")
            } footer: {
                Text("开启后使用 openkey_fuzzy 方案（Phase 1 近似：后续在 core 侧以 schema patch 落地）。")
            }

            Section {
                Toggle("启动时使用英文模式", isOn: $model.asciiMode)
            } header: {
                Text("中英切换")
            } footer: {
                Text("输入过程中可用 Shift 临时切换中/英（rime ascii_mode）。")
            }
        }
        .task {
            await model.load()
        }
    }
}
