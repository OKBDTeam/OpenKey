// CredentialProviderViewController —— 自动填充扩展骨架（docs/02 4.5.4）。
//
// Phase 4 落地：读取本地 vault（openkey-vault 的 LocalVault）中的登录条目，
// 经 ASCredentialProviderViewController 展示并回填。本里程碑只保留可编译骨架：
// 列表占位 + 明确的 TODO 标注，避免在 vault C ABI 尚未开放时虚构功能。

import AuthenticationServices
import UIKit

final class CredentialProviderViewController: ASCredentialProviderViewController {
    /// TODO(Phase 4)：接入 openkey-vault 的 C ABI（ok_vault_*），按服务与账号查询。
    private var placeholderIdentities: [String] = [
        "（待接入本地 vault）",
    ]

    override func prepareCredentialList(for serviceIdentifiers: [ASCredentialServiceIdentifier]) {
        // 骨架：v0.1.0 不接入 vault（Phase 4），直接结束本次请求。
        // 待实现：按 serviceIdentifiers 查询本地 vault 条目，
        // 以 extensionContext.completeRequest(withSelectedCredential:) 回填。
        cancelRequest()
    }

    @objc private func cancelRequest() {
        extensionContext.cancelRequest(withError: NSError(domain: ASExtensionErrorDomain,
                                                          code: ASExtensionError.userCanceled.rawValue))
    }
}
