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
        // 骨架：不查询 vault，仅登记占位项。
        // 注：serviceIdentifier 为可选；无服务上下文时用通配标识。
        let fallback = ASCredentialServiceIdentifier(
            identifier: "OpenKey Placeholder",
            type: ASCredentialServiceIdentifier.ASAPIClientCredentialType
        )
        for (index, title) in placeholderIdentities.enumerated() {
            extensionContext.completeRequest(
                withSelectedCredential: ASPasswordCredentialIdentity(
                    serviceIdentifier: serviceIdentifiers.first ?? fallback,
                    user: title,
                    recordIdentifier: String(index)
                )
            )
        }
    }

    @objc private func cancelRequest() {
        extensionContext.cancelRequest(withError: NSError(domain: ASExtensionErrorDomain,
                                                          code: ASExtensionError.userCanceled.rawValue))
    }
}
