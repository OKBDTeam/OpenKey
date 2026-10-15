// OpenKeyApp —— 开门输入法 iOS 主 App（docs/02 4.5.3）。
//
// 职责：输入法启用引导、设置（方案/双拼/模糊音/中英切换）、关于与版本。
// 设置经 OpenKeyCore 落到 librime 设置（sync_entries 的 setting 条目由
// core 持久化到主 App 容器；App Group 共享留待 Phase 3 同步接线）。

import SwiftUI

@main
struct OpenKeyApp: App {
    var body: some Scene {
        WindowGroup {
            RootView()
        }
    }
}

struct RootView: View {
    var body: some View {
        TabView {
            SettingsView()
                .tabItem { Label("设置", systemImage: "gearshape") }
            EnableGuideView()
                .tabItem { Label("启用", systemImage: "keyboard") }
            AboutView()
                .tabItem { Label("关于", systemImage: "info.circle") }
        }
    }
}

/// 启用引导：跳转系统设置并提示添加键盘。
struct EnableGuideView: View {
    var body: some View {
        List {
            Section {
                Text("请在「设置 → 通用 → 键盘 → 键盘 → 添加新键盘」中选择「开门输入法」。")
                    .font(.callout)
                Button("打开系统设置") {
                    if let url = URL(string: UIApplication.openSettingsURLString) {
                        UIApplication.shared.open(url)
                    }
                }
            }
            Section("当前状态") {
                LabeledContent("Full Access", value: "未开启（本版本无需联网）")
                Text("未开启 Full Access 时键盘扩展无法访问共享容器与网络；本版本词库与方案随 App 内置，输入功能完整可用。")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }
            Section("内存预算") {
                Text("键盘扩展常驻内存目标 ≤40MB（docs/02 4.5.2）。本版本 core 随扩展直链，词库表由 librime 按需映射；若后续实测超限，将按设计改为「主 App 内 core + App Group 只读共享」。")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }
        }
        .navigationTitle("启用输入法")
    }
}

struct AboutView: View {
    var body: some View {
        List {
            Section {
                LabeledContent("版本", value: appVersion)
                LabeledContent("core", value: OpenKeyEngine.version)
            }
            Section("说明") {
                Text("开门输入法是开源五端中文拼音输入法，核心基于 librime。你的输入法，你的服务器：默认不上传任何数据，词库、同步与设置全部留在本机。")
                    .font(.callout)
            }
        }
        .navigationTitle("关于")
    }

    private var appVersion: String {
        let info = Bundle.main.infoDictionary
        let short = info?["CFBundleShortVersionString"] as? String ?? "0.1.0"
        let build = info?["CFBundleVersion"] as? String ?? "1"
        return "\(short) (\(build))"
    }
}
