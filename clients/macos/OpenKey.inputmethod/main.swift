// 输入法 bundle 的进程入口。
//
// 工程形态为「application 产品类型 + WRAPPER_EXTENSION=inputmethod」（README
// 有与 IMKSwift 方案的偏差说明）：系统以后台应用方式拉起输入法，AppKit 依据
// Info.plist 的 NSPrincipalClass（NSManualApplication）创建实例，其 init 中挂
// AppDelegate 并创建 IMKServer。此处只提供 C 入口 main 供链接器解析，
// 不参与输入法逻辑。

import AppKit

let application = NSApplication.shared
// 输入法不显示在 Dock、不抢焦点（Info.plist 的 LSBackgroundOnly 亦已声明）。
application.setActivationPolicy(.prohibited)
application.run()
