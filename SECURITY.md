# 安全策略（Security Policy）

OpenKey（开门输入法）高度重视安全。本文件说明漏洞上报流程、受支持版本与密钥/数据边界摘要。
安全设计的完整论证见 `docs/02-技术设计与实施方案.md` 第 6 章（本文不重复展开，只作索引）。

## 报告漏洞

- **安全邮箱**：`security@openkey.im`（**占位地址，待项目域名就绪后更新——TODO**）
- 或使用 GitHub 的 [Private Vulnerability Reporting](https://docs.github.com/en/code-security/security-advisories/guidance-on-reporting-and-writing-information-about-vulnerabilities/privately-reporting-a-security-vulnerability)（私密上报，不公开细节）。
- **请勿在公开 issue / 讨论区提交漏洞细节**，包括但不限于：vault 解密路径、同步 E2E 实现、
  IME 框架注入、组件校验逻辑。
- 上报时请附：影响端（Windows/macOS/Linux/Android/iOS/core）、复现步骤、影响评估；
  如方便，请给出修复建议。我们会在 **72 小时内**确认收到并给出初步评估，处理期间每 7 天同步进展。
- 修复发布后，如双方同意，将在发布说明中致谢报告者。

## 支持版本

| 版本 | 支持状态 | 说明 |
|---|---|---|
| 0.1.x（当前开发线） | ✅ 积极修复 | 随取随修；安全修复优先于功能 |
| < 0.1 | ❌ 不受支持 | 未发布版本，无补丁义务 |

正式发版（tag `v*`）后的支持窗口以 `.github/workflows/release.yml` 实际发布记录为准。

## 密钥层级与数据边界（摘要）

详见 `docs/02-技术设计与实施方案.md` 第 6 章，要点如下：

- **密钥层级（6.2）**：主密码 → KDF（Argon2id / PBKDF2）→ master key → HKDF 派生
  *vault user key*（vault 条目加密）与 *sync passphrase*（同步 E2E）；
  组件校验公钥为**内置指纹、非派生**。
- **OS 安全区（6.2）**：user key 包裹件存 Keychain / Android Keystore / CNG DPAPI / libsecret；
  包裹密钥不出安全区；内存中 key 材料使用 zeroize 清零。
- **密码模式铁律（5.2）**：密码模式期间暂停一切学习与记录（词频、剪贴板、候选上报）；
  密码字段值永不出现在任何日志 / 同步通道。
- **同步 E2E（6.3）**：AES-256-GCM（HKDF 派生密钥，AAD=key|kind|hlc）；
  无 passphrase 时仅 TLS，且设置页红色警示"服务器可见明文"。
- **组件供应链（6.4）**：SHA-256 + manifest GPG 签名 + 客户端内置公钥指纹；
  发布私钥**离线保管**，CI 只产出未签名 manifest（见 `shared/components/README.md`）。
- **数据边界**：本地优先、无默认上传、无遥测、无账号体系——完整数据清单见 `PRIVACY.md`。

## 已知未决项（TODO）

- 安全邮箱域名（`openkey.im`）尚未注册，当前为占位。
- Windows Authenticode / macOS Developer ID + 公证 / Android APK 签名证书尚未配置（DR-002），
  发布产物为未签名版本。
- 渗透测试纳入密码功能里程碑验收（`docs/02` 风险登记册 R-07）。
