// SettingsView —— iOS 端设置页（docs/02 4.2.1 设置项的 iOS 形态）。
//
// 设置项：输入方案、双拼方案、模糊音、中英切换。写入经 OpenKeySettingsStore
// 落到 openkey.db 的 setting 条目（docs/02 3.2），键盘扩展下次读库即生效。
// 双拼与模糊音在 Phase 1 经「切换 schema」近似落地（OpenKeySettingsStore
// 的 effectiveSchemaId 给出当前生效方案）；更细分项留待后续里程碑。

import SwiftUI
import OpenKeyCore

struct SettingsView: View {
    @StateObject private var model = SettingsModel()

    var body: some View {
        NavigationStack {
            Form {
                Section("输入方案") {
                    if model.schemas.isEmpty {
                        HStack {
                            ProgressView()
                            Text(model.isLoading ? "正在加载方案…" : model.loadHint)
                                .foregroundStyle(.secondary)
                        }
                    } else {
                        Picker("方案", selection: $model.selectedSchemaId) {
                            ForEach(model.schemas) { schema in
                                Text(schema.name).tag(schema.id)
                            }
                        }
                    }
                }

                Section("双拼") {
                    Picker("双拼方案", selection: $model.doublePinyin) {
                        ForEach(DoublePinyinScheme.allCases, id: \.self) { scheme in
                            Text(scheme.title).tag(scheme)
                        }
                    }
                }

                Section("模糊音") {
                    Toggle("启用模糊音", isOn: $model.fuzzyPinyin)
                    Text("模糊音以独立方案（openkey_fuzzy）实现：开启后启用平舌翘舌、鼻音与前后鼻音的模糊派生。")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                }

                Section("中英切换") {
                    Toggle("英文模式", isOn: $model.asciiMode)
                    Text("键盘上的 Shift 为临时切换，此处为持久设置。")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                }

                Section("当前生效") {
                    LabeledContent("方案 ID", value: model.effectiveSchemaId)
                    if let error = model.errorText {
                        Text(error).font(.footnote).foregroundStyle(.red)
                    }
                }
            }
            .navigationTitle("设置")
            .onAppear { model.load() }
            .onChange(of: model.selectedSchemaId) { _ in model.applySchema() }
            .onChange(of: model.doublePinyin) { _ in model.applySchema() }
            .onChange(of: model.fuzzyPinyin) { _ in model.applySchema() }
            .onChange(of: model.asciiMode) { _ in model.applySchema() }
        }
    }
}

/// 设置页状态模型：持有引擎，读写设置并在变更时切换会话方案。
final class SettingsModel: ObservableObject {
    @Published var schemas: [OpenKeySchemaInfo] = []
    @Published var selectedSchemaId: String = OpenKeySchemaId.pinyin
    @Published var doublePinyin: DoublePinyinScheme = .off
    @Published var fuzzyPinyin: Bool = false
    @Published var asciiMode: Bool = false
    @Published var isLoading = false
    @Published var errorText: String?

    private var engine: OpenKeyEngine?
    private var store: OpenKeySettingsStore?

    var effectiveSchemaId: String {
        store?.effectiveSchemaId() ?? selectedSchemaId
    }

    var loadHint: String {
        errorText.map { "加载失败：\($0)" } ?? "未找到已部署方案"
    }

    func load() {
        guard engine == nil, !isLoading else { return }
        isLoading = true
        defer { isLoading = false }

        let support = FileManager.default.urls(for: .applicationSupportDirectory,
                                               in: .userDomainMask).first!
        let user = support.appendingPathComponent("openkey").path
        try? FileManager.default.createDirectory(atPath: user, withIntermediateDirectories: true)
        // 共享数据（方案与词库）随 App 内置于 SharedData/，由打包脚本写入。
        let shared = Bundle.main.bundleURL.appendingPathComponent("SharedData").path

        guard let engine = OpenKeyEngine(sharedDataDir: shared,
                                         userDataDir: user,
                                         dataDbPath: user + "/openkey.db",
                                         componentsDir: user + "/components") else {
            errorText = OpenKeyEngine.lastGlobalError()
            return
        }
        self.engine = engine
        let store = OpenKeySettingsStore(engine: engine)
        self.store = store

        var list: [OpenKeySchemaInfo] = []
        var index = 0
        while let schema = engine.schema(at: index) {
            list.append(schema)
            index += 1
        }
        schemas = list
        selectedSchemaId = store.schemaId ?? list.first?.id ?? OpenKeySchemaId.pinyin
        doublePinyin = store.doublePinyin
        fuzzyPinyin = store.fuzzyPinyin
        asciiMode = store.asciiMode
    }

    /// 方案变更：写入设置并切换一个临时会话（librime 会话级 schema 切换）。
    func applySchema() {
        guard let store else { return }
        store.update(schemaId: selectedSchemaId)
        store.update(doublePinyin: doublePinyin)
        store.update(fuzzyPinyin: fuzzyPinyin)
        if let session = engine?.makeSession() {
            _ = session.selectSchema(store.effectiveSchemaId())
        }
    }
}
