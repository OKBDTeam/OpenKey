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
        // 骨架：不查询 vault，仅展示占位项。
        let identities = placeholderIdentities.enumerated().map { index, title in
            ASPasswordCredentialIdentity(
                serviceIdentifier: serviceIdentifiers.first,
                user: title,
                recordIdentifier: String(index)
            )
        }
        extensionContext.completeRequest(withSelectedCredentialIdentities: identities)
    }

    override func prepareInterface(forExtensionContext: ASCredentialExtensionContext) {
        // 骨架：最小界面（占位列表 + 说明）。Phase 4 替换为 vault 条目列表。
        view.backgroundColor = .systemBackground
        let label = UILabel()
        label.text = "开门密码\n自动填充将在 Phase 4 接入本地 vault 后启用。"
        label.numberOfLines = 0
        label.textAlignment = .center
        label.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(label)
        NSLayoutConstraint.activate([
            label.centerXAnchor.constraint(equalTo: view.centerXAnchor),
            label.centerYAnchor.constraint(equalTo: view.centerYAnchor),
            label.widthAnchor.constraint(lessThanOrEqualTo: view.widthAnchor, constant: -48),
        ])

        let cancel = UIButton(type: .system)
        cancel.setTitle("取消", for: .normal)
        cancel.addTarget(self, action: #selector(cancelRequest), for: .touchUpInside)
        cancel.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(cancel)
        NSLayoutConstraint.activate([
            cancel.topAnchor.constraint(equalTo: label.bottomAnchor, constant: 16),
            cancel.centerXAnchor.constraint(equalTo: view.centerXAnchor),
        ])
    }

    @objc private func cancelRequest() {
        extensionContext.cancelRequest(withError: NSError(domain: ASExtensionErrorDomain,
                                                          code: ASExtensionError.userCanceled.rawValue))
    }
}
