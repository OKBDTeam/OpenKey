#!/usr/bin/env python3
"""make_versions.py —— 由 CI 发布产物生成 versions.json（docs/02-技术设计与实施方案.md 附录 B）。

用法：
    python3 scripts/release/make_versions.py \
        --artifacts-dir staging \
        --release-dir dist \
        --tag v0.1.0 \
        [--repo leic4u/OpenKey] \
        [--channel stable]

行为：
  1. 若存在分平台 core 产物（OpenKey-core-<ver>-<platform>-<arch>.zip），合并为单一
     组件包 OpenKey-core-<ver>.zip（内部按平台分目录），对应 core-rust 条目。
  2. 按文件名规则识别六类组件 app-windows / app-macos / app-linux / app-android /
     app-ios / core-rust，以及可选的 lexicon-openkey-base（词库包，CC0）；
     被引用的文件复制到 --release-dir，未识别文件仅告警不引用。
  3. 计算每个被引用文件的 SHA-256 与字节数，写出 versions.json：
     schema=1、published_at=UTC 当前时间、channel=stable。
     **不含 signature 字段** —— manifest 的 GPG 签名由发布私钥离线完成
     （docs/02 6.4，见 shared/components/README.md），CI 只产出未签名 manifest。
  4. 写出 sha256sums.txt（含 versions.json 自身，便于整体校验）。

仅依赖 Python 3 标准库。
"""

from __future__ import annotations

import argparse
import datetime
import hashlib
import json
import re
import shutil
import sys
import zipfile
from pathlib import Path

# 六类必备组件 + 可选词库包。value 为许可（附录 B 的 license 字段）。
COMPONENTS = {
    "app-windows": "GPL-3.0",           # DR-001：客户端整体 GPL-3.0
    "app-macos": "GPL-3.0",
    "app-linux": "GPL-3.0",
    "app-android": "GPL-3.0",
    "app-ios": "GPL-3.0",
    "core-rust": "GPL-3.0-or-later",    # core workspace license
    # 可选：openkey-lexicon 产出实际词库文件时才出现（DR-001：CC0/自制）
    "lexicon-openkey-base": "CC0-1.0",
}
REQUIRED_COMPONENTS = [
    "app-windows",
    "app-macos",
    "app-linux",
    "app-android",
    "app-ios",
    "core-rust",
]

# 同一组件存在多个候选文件时的优先顺序（越靠前越优先）。
PREFERENCE = {
    "app-windows": [".exe", ".zip"],
    "app-linux": [".deb"],
    "app-android": [".apk"],
    "app-ios": [".zip"],
    "app-macos": [".zip"],
    "core-rust": [".zip"],
    "lexicon-openkey-base": [".zip"],
}

# 分平台 core 产物：OpenKey-core-0.1.0-linux-x86_64.zip（release.yml build-core 产出）
CORE_PLATFORM_RE = re.compile(r"^OpenKey-core-.+-(linux|macos|windows)-.+\.zip$")


def sha256_of(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def merge_core_bundles(staging: Path, release_dir: Path, version: str) -> Path | None:
    """把各平台 core zip 合并为 OpenKey-core-<version>.zip；无分平台产物时返回 None。"""
    out = release_dir / f"OpenKey-core-{version}.zip"
    srcs = sorted(p for p in staging.glob("OpenKey-core-*.zip") if CORE_PLATFORM_RE.match(p.name))
    if not srcs:
        return None
    release_dir.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as z:
        for s in srcs:
            platform = CORE_PLATFORM_RE.match(s.name).group(1)
            z.write(s, arcname=f"{platform}/{s.name}")
    print(f"[core-rust] 合并 {len(srcs)} 个平台产物 -> {out.name}")
    return out


def classify(path: Path, version: str) -> str | None:
    """按文件名识别组件名；无法识别返回 None。"""
    name = path.name
    lower = name.lower()
    suffix = path.suffix.lower()
    if suffix == ".deb":
        return "app-linux"
    if suffix == ".apk":
        return "app-android"
    if suffix == ".exe":
        return "app-windows"
    if suffix == ".zip":
        if name == f"OpenKey-core-{version}.zip":
            return "core-rust"
        if "ios" in lower:
            return "app-ios"
        if "macos" in lower or "darwin" in lower:
            return "app-macos"
        if "windows" in lower or "win" in lower:
            return "app-windows"
        if "lexicon" in lower:
            return "lexicon-openkey-base"
    return None


def pick_preferred(component: str, candidates: list[Path]) -> Path:
    order = PREFERENCE.get(component, [])
    def key(p: Path) -> int:
        try:
            return order.index(p.suffix.lower())
        except ValueError:
            return len(order)
    return sorted(candidates, key=key)[0]


def main() -> int:
    parser = argparse.ArgumentParser(description="生成 versions.json（docs/02 附录 B）")
    parser.add_argument("--artifacts-dir", required=True, type=Path,
                        help="CI 下载的全部构建产物目录（扁平）")
    parser.add_argument("--release-dir", required=True, type=Path,
                        help="发布资产输出目录（被引用文件 + versions.json + sha256sums.txt）")
    parser.add_argument("--tag", required=True, help="tag 名，如 v0.1.0")
    parser.add_argument("--repo", default="leic4u/OpenKey",
                        help="GitHub 仓库 owner/name（用于拼接下载 URL）")
    parser.add_argument("--channel", default="stable", help="发布通道（默认 stable）")
    args = parser.parse_args()

    tag = args.tag.lstrip("v")
    version = tag
    base_url = f"https://github.com/{args.repo}/releases/download/{args.tag}"
    repo_url = f"https://github.com/{args.repo}"
    published_at = datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")

    staging: Path = args.artifacts_dir
    release_dir: Path = args.release_dir
    if not staging.is_dir():
        print(f"error: artifacts 目录不存在：{staging}", file=sys.stderr)
        return 1
    release_dir.mkdir(parents=True, exist_ok=True)

    # 1) 合并分平台 core 产物（产物写入 release_dir，需显式加入候选）
    merged_core = merge_core_bundles(staging, release_dir, version)

    # 2) 识别组件文件
    candidates: dict[str, list[Path]] = {}
    unclassified: list[str] = []
    if merged_core is not None:
        candidates.setdefault("core-rust", []).append(merged_core)
    for p in sorted(staging.iterdir()):
        if not p.is_file():
            continue
        if CORE_PLATFORM_RE.match(p.name):
            continue  # 已并入 core-rust 组件包
        component = classify(p, version)
        if component is None:
            unclassified.append(p.name)
            continue
        candidates.setdefault(component, []).append(p)

    # 3) 复制被引用文件并计算哈希
    components: dict[str, dict] = {}
    for component in list(COMPONENTS):
        files = candidates.get(component)
        if not files:
            continue
        chosen = pick_preferred(component, files)
        if len(files) > 1:
            ignored = ", ".join(f.name for f in files if f != chosen)
            print(f"[warn] {component} 存在多个候选，引用 {chosen.name}，忽略：{ignored}")
        dst = release_dir / chosen.name
        if chosen.resolve() != dst.resolve():
            shutil.copyfile(chosen, dst)
        components[component] = {
            "version": version,
            "url": f"{base_url}/{chosen.name}",
            "sha256": sha256_of(dst),
            "size": dst.stat().st_size,
            "license": COMPONENTS[component],
            "source": repo_url,
            "channel": args.channel,
        }
        print(f"[ok] {component}: {chosen.name} ({dst.stat().st_size} bytes)")

    missing = [c for c in REQUIRED_COMPONENTS if c not in components]
    if missing:
        print(f"[warn] 缺少组件条目（对应端构建未就绪或跳过）：{', '.join(missing)}")
    for name in unclassified:
        print(f"[warn] 未识别文件（不引用）：{name}")

    # 4) versions.json（不含 signature：GPG 签名由维护者离线完成，docs/02 6.4）
    manifest = {
        "schema": 1,
        "published_at": published_at,
        "components": components,
    }
    versions_path = release_dir / "versions.json"
    with versions_path.open("w", encoding="utf-8", newline="\n") as f:
        json.dump(manifest, f, ensure_ascii=False, indent=2)
        f.write("\n")
    print(f"[ok] 写入 {versions_path}")

    # 5) sha256sums.txt（含 versions.json 自身）
    sums_path = release_dir / "sha256sums.txt"
    with sums_path.open("w", encoding="utf-8", newline="\n") as f:
        for p in sorted(release_dir.iterdir()):
            if p.is_file() and p.name not in ("sha256sums.txt", "release-notes.md"):
                f.write(f"{sha256_of(p)}  {p.name}\n")
    print(f"[ok] 写入 {sums_path}")

    if missing:
        print(f"notice: versions.json 已生成，但缺少 {len(missing)} 个必备组件条目", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
