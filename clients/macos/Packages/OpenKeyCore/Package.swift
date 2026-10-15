// swift-tools-version:5.9
import PackageDescription

// OpenKeyCore —— 五端客户端（当前 macOS / iOS）共用的 core 绑定包。
//
// 结构：
//   COpenKey    C 目标：include/openkey.h（core/ffi-headers/openkey.h 的拷贝，cbindgen 产物）
//               + include/module.modulemap（显式模块映射）+ shim.c（占位编译单元）。
//               头文件与 modulemap 由 scripts/build-bundle.sh 从 core 同步（勿手改 openkey.h）。
//   OpenKeyCore Swift 目标：对 openkey.h C ABI 的类型安全封装（引擎/会话/设置/keysym 映射）。
//
// 平台：macOS 13+ / iOS 16+（docs/02 2.5 技术栈基线）。
let package = Package(
    name: "OpenKeyCore",
    platforms: [
        .macOS(.v13),
        .iOS(.v16),
    ],
    products: [
        // 同时暴露 Swift 封装与 C 模块（需要直接 `import COpenKey` 时可用了）。
        .library(name: "OpenKeyCore", targets: ["OpenKeyCore", "COpenKey"]),
    ],
    targets: [
        .target(
            name: "COpenKey",
            path: "Sources/COpenKey"
            // publicHeadersPath 默认为 "include"：include/openkey.h 与 include/module.modulemap 所在目录。
        ),
        .target(
            name: "OpenKeyCore",
            dependencies: ["COpenKey"],
            path: "Sources/OpenKeyCore"
        ),
    ]
)
