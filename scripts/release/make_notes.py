#!/usr/bin/env python3
"""make_notes.py —— 生成 Markdown 发布说明（conventional commits 分组 + 固定段落）。

用法：
    python3 scripts/release/make_notes.py --tag v0.1.0 \
        [--repo leic4u/OpenKey] [--range v0.0.9..v0.1.0] [--out RELEASE_NOTES.md]

行为：
  - 取 <上一 tag>..<当前 tag> 的提交（--range 可显式指定；未指定时按
    `git tag --sort=-creatordate` 推断上一 tag；无上一 tag 则列全部历史）。
  - 按 conventional commits 的 type 分组（feat/fix/docs/...），未匹配的进"其他变更"。
  - 追加固定段落：安装说明、已知限制（iOS 未签名 / Windows 未代码签名 /
    librime 各端版本 / DR-002 分发说明）、校验和提示。

仅依赖 Python 3 标准库；需在 git 仓库内运行（CI 中 checkout 带 fetch-depth: 0）。
"""

from __future__ import annotations

import argparse
import datetime
import re
import subprocess
import sys

TYPE_TITLES = [
    ("feat", "新功能"),
    ("fix", "问题修复"),
    ("perf", "性能优化"),
    ("refactor", "重构"),
    ("docs", "文档"),
    ("test", "测试"),
    ("build", "构建"),
    ("ci", "持续集成"),
    ("chore", "日常维护"),
    ("style", "代码风格"),
    ("revert", "变更回滚"),
]
OTHER = "其他变更"

# conventional commit 前缀：type(scope)!: subject
SUBJECT_RE = re.compile(r"^(?P<type>[A-Za-z]+)(?:\((?P<scope>[^)]*)\))?(?P<bang>!)?:\s*(?P<subject>.+)$")


def git(*args: str) -> str:
    proc = subprocess.run(["git", *args], capture_output=True, text=True)
    if proc.returncode != 0:
        raise RuntimeError(f"git {' '.join(args)} 失败：{proc.stderr.strip()}")
    return proc.stdout


def previous_tag(current: str) -> str | None:
    tags = git("tag", "--sort=-creatordate").split()
    tags = [t for t in tags if t]
    if current not in tags:
        return None
    idx = tags.index(current)
    if idx + 1 < len(tags):
        return tags[idx + 1]
    return None


def collect_commits(rev_range: str) -> list[tuple[str, str]]:
    out = git("log", "--no-merges", "--format=%H%x1f%s", rev_range)
    commits = []
    for line in out.splitlines():
        if not line.strip():
            continue
        sha, _, subject = line.partition("\x1f")
        commits.append((sha.strip(), subject.strip()))
    return commits


def group_commits(commits: list[tuple[str, str]]) -> list[tuple[str, list[tuple[str, str, str]]]]:
    """返回 [(分组标题, [(type, subject, sha), ...]), ...]，按固定顺序，空组省略。"""
    groups: dict[str, list[tuple[str, str, str]]] = {}
    for sha, subject in commits:
        m = SUBJECT_RE.match(subject)
        type_ = m.group("type").lower() if m else ""
        text = m.group("subject") if m else subject
        groups.setdefault(type_, []).append((type_, text, sha))
    ordered = []
    for type_, title in TYPE_TITLES:
        if type_ in groups:
            ordered.append((title, groups.pop(type_)))
    if groups:
        leftovers = [item for items in groups.values() for item in items]
        ordered.append((OTHER, leftovers))
    return ordered


def render(tag: str, version: str, repo: str, rev_range: str, grouped, total: int) -> str:
    repo_url = f"https://github.com/{repo}"
    release_url = f"{repo_url}/releases/tag/{tag}"
    today = datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%d")
    lines: list[str] = []
    lines.append(f"# OpenKey {version}")
    lines.append("")
    lines.append(f"> 发布日期：{today} · 标签：`{tag}` · 下载：[GitHub Releases]({release_url})")
    lines.append(f"> 本次发布共 {total} 个提交（不含 merge，范围 `{rev_range}`）")
    lines.append("")
    lines.append("## 更新内容")
    lines.append("")
    if not grouped:
        lines.append("- 本次发布范围内没有提交记录。")
        lines.append("")
    for title, items in grouped:
        lines.append(f"### {title}")
        lines.append("")
        for _type, subject, sha in items:
            short = sha[:7]
            lines.append(f"- {subject}（[`{short}`]({repo_url}/commit/{sha})）")
        lines.append("")

    lines.append("## 安装说明")
    lines.append("")
    lines.append("### Windows")
    lines.append("")
    lines.append(f"下载 `OpenKey-{version}-windows-x86_64.zip`，解压后运行其中的安装程序 / 主程序。")
    lines.append("**当前构建未代码签名**：SmartScreen 提示“Windows 已保护你的电脑”时，选择“更多信息 → 仍要运行”。")
    lines.append("")
    lines.append("### macOS")
    lines.append("")
    lines.append(f"下载 `OpenKey-{version}-macos-arm64.zip`，解压后将 `.app` 拖入“应用程序”。")
    lines.append("**当前构建未公证**：首次打开请右键（或按住 Control 单击）→“打开”，"
                 "或在“系统设置 → 隐私与安全性”中点击“仍要打开”。")
    lines.append("")
    lines.append("### Linux（Debian / Ubuntu）")
    lines.append("")
    lines.append(f"`sudo dpkg -i OpenKey-{version}-linux-x86_64.deb`，随后重新登录或重启 fcitx5 加载输入法。")
    lines.append("")
    lines.append("### Android")
    lines.append("")
    lines.append(f"下载 `OpenKey-{version}-android-release-unsigned.apk` 侧载（**未使用发布密钥签名**，"
                 "debug 签名产物同理）：需先在系统设置中允许该来源“安装未知应用”；"
                 "然后在“设置 → 语言与输入法”中启用 OpenKey 并设为当前输入法。")
    lines.append("")
    lines.append("### iOS")
    lines.append("")
    lines.append(f"当前仅提供模拟器构建 `OpenKey-{version}-ios-simulator.zip`（**未签名，无法直接安装到真机**）。"
                 "真机分发待 TestFlight 公测通道配置（DR-002）。")
    lines.append("")

    lines.append("## 已知限制")
    lines.append("")
    lines.append("- **iOS**：TestFlight 分发通道待配置；CI 产物为模拟器构建且未签名"
                 "（DR-002：iOS 在非越狱条件下无合法自装通道，近期以 TestFlight 承载，排为最后交付端）。")
    lines.append("- **Windows / macOS**：未代码签名 / 未公证（证书待配置，DR-002）；"
                 "SmartScreen / Gatekeeper 对未签名程序的拦截属预期现象。")
    lines.append("- **librime**：桌面端锁定 librime 1.17.0（`core/vendor/librime` submodule）；"
                 "Android / iOS 的 librime 链接方案待定，对应端 core 暂以 `--no-default-features` 构建。")
    lines.append("- **分发**：本项目不上架任何应用商店，全部经官网 + GitHub Releases 自行分发（DR-002）；"
                 "`versions.json` 的 GPG 签名由发布私钥离线完成（docs/02 6.4），CI 产出为未签名 manifest。")
    lines.append("")

    lines.append("## 校验和")
    lines.append("")
    lines.append("所有产物的 SHA-256 见同页 `versions.json` 与 `sha256sums.txt`：")
    lines.append("")
    lines.append("```sh")
    lines.append("# Linux / macOS")
    lines.append("sha256sum -c sha256sums.txt")
    lines.append("")
    lines.append("# Windows PowerShell")
    lines.append("Get-FileHash -Algorithm SHA256 <文件>")
    lines.append("```")
    lines.append("")
    return "\n".join(lines)


def main() -> int:
    parser = argparse.ArgumentParser(description="生成 Markdown 发布说明")
    parser.add_argument("--tag", required=True, help="当前 tag，如 v0.1.0")
    parser.add_argument("--repo", default="leic4u/OpenKey", help="GitHub 仓库 owner/name")
    parser.add_argument("--range", dest="rev_range", default=None,
                        help="git 提交范围，如 v0.0.9..v0.1.0；缺省按上一 tag 推断")
    parser.add_argument("--out", default=None, help="输出文件；缺省打印到 stdout")
    args = parser.parse_args()

    version = args.tag.lstrip("v")
    try:
        rev_range = args.rev_range
        if not rev_range:
            prev = previous_tag(args.tag)
            rev_range = f"{prev}..{args.tag}" if prev else args.tag
        commits = collect_commits(rev_range)
    except RuntimeError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 1

    grouped = group_commits(commits)
    notes = render(args.tag, version, args.repo, rev_range, grouped, len(commits))

    if args.out:
        with open(args.out, "w", encoding="utf-8", newline="\n") as f:
            f.write(notes)
        print(f"已写入 {args.out}（范围 {rev_range}，{len(commits)} 个提交）")
    else:
        print(notes)
    return 0


if __name__ == "__main__":
    sys.exit(main())
