#!/bin/sh
# clients/macos/scripts/build-bundle.sh —— OpenKey macOS 输入法打包/安装脚本（说明性脚本）。
#
# 职责（对应 README「构建」一节）：
#   1. 从 core/ffi-headers/openkey.h 同步 C 头文件到 SwiftPM 包（cbindgen 产物，勿手改拷贝）；
#   2. xcodegen generate + xcodebuild（Release）构建设置 App 与输入法 bundle；
#   3. 把 librime.dylib 拷入各 bundle 的 Contents/Frameworks，并改写 install name 为 @rpath；
#   4. 把 shared/schemas 部署到用户数据目录（~/Library/Application Support/OpenKey/shared）；
#   5. 可选：安装到 /Library/Input Methods、Developer ID 签名与公证（见文末 TODO）。
#
# 用法：
#   scripts/build-bundle.sh            # 仅构建 + 打包修正
#   scripts/build-bundle.sh --install  # 额外安装输入法到 /Library/Input Methods（需要 sudo）
#
# 注意：CI 不运行本脚本（CI 只验证 xcodegen + xcodebuild 可构建）。
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
MACOS_DIR=$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)
REPO_DIR=$(CDPATH= cd -- "$MACOS_DIR/../.." && pwd)
CONFIGURATION="${CONFIGURATION:-Release}"
INSTALL=0
[ "${1:-}" = "--install" ] && INSTALL=1

cd "$MACOS_DIR"

# ---------------------------------------------------------------------------
# 1. 同步 C 头文件（core/ffi-headers/openkey.h → Packages/OpenKeyCore/Sources/COpenKey/include/）
# ---------------------------------------------------------------------------
HEADER_SRC="$REPO_DIR/core/ffi-headers/openkey.h"
HEADER_DST="$MACOS_DIR/Packages/OpenKeyCore/Sources/COpenKey/include/openkey.h"
if [ -f "$HEADER_SRC" ]; then
  if ! cmp -s "$HEADER_SRC" "$HEADER_DST"; then
    cp "$HEADER_SRC" "$HEADER_DST"
    echo "[1/5] 已同步 openkey.h（$HEADER_SRC -> $HEADER_DST）"
  else
    echo "[1/5] openkey.h 已是最新"
  fi
else
  echo "[1/5] notice: 未找到 $HEADER_SRC，跳过同步（core 未初始化时正常）"
fi

# ---------------------------------------------------------------------------
# 2. 生成工程并构建
# ---------------------------------------------------------------------------
echo "[2/5] xcodegen generate"
xcodegen generate

echo "[2/5] xcodebuild ($CONFIGURATION)"
xcodebuild -project OpenKey.xcodeproj -scheme OpenKey \
  -configuration "$CONFIGURATION" -derivedDataPath build \
  CODE_SIGNING_ALLOWED=NO build

PRODUCTS_DIR="$MACOS_DIR/build/Build/Products/$CONFIGURATION"
APP="$PRODUCTS_DIR/OpenKey.app"
IM="$PRODUCTS_DIR/OpenKeyInputMethod.inputmethod"
if [ ! -d "$APP" ] || [ ! -d "$IM" ]; then
  echo "error: 构建产物不完整（$APP / $IM）" >&2
  exit 1
fi

# ---------------------------------------------------------------------------
# 3. librime.dylib 嵌入与 install name 修正
#    CI 中 librime 以绝对路径 install name 安装（ci-clients.yml 有 install_name_tool -id），
#    直接分发会因绝对路径失效而无法加载，这里统一改写为 @rpath 并放入 bundle 内。
# ---------------------------------------------------------------------------
echo "[3/5] 嵌入 librime.dylib"
RIME_LIB="$MACOS_DIR/core-binding/librime.dylib"
for BUNDLE in "$APP" "$IM"; do
  if [ ! -f "$RIME_LIB" ]; then
    echo "notice: 未找到 $RIME_LIB，跳过 dylib 嵌入（core-binding 未就绪）"
    break
  fi
  FRAMEWORKS="$BUNDLE/Contents/Frameworks"
  mkdir -p "$FRAMEWORKS"
  cp "$RIME_LIB" "$FRAMEWORKS/librime.dylib"
  install_name_tool -id "@rpath/librime.dylib" "$FRAMEWORKS/librime.dylib" 2>/dev/null || true
  BINARY="$BUNDLE/Contents/MacOS/$(defaults read "$BUNDLE/Contents/Info.plist" CFBundleExecutable 2>/dev/null || basename "$BUNDLE" .app)"
  if [ -x "$BINARY" ]; then
    # 把链接期记录的绝对路径引用改到 bundle 内相对路径。
    OLD_REF=$(otool -L "$BINARY" 2>/dev/null | awk '/librime\.dylib/ && !/@rpath/ {print $1; exit}')
    if [ -n "${OLD_REF:-}" ]; then
      install_name_tool -change "$OLD_REF" "@rpath/librime.dylib" "$BINARY" || true
      echo "    已改写 $BUNDLE 的 librime 引用：$OLD_REF -> @rpath/librime.dylib"
    fi
  fi
done

# ---------------------------------------------------------------------------
# 4. 部署 rime 共享数据（shared/schemas → 用户数据目录）
#    输入法与设置 App 的引擎 sharedDataDir 均指向 ~/Library/Application Support/OpenKey/shared。
# ---------------------------------------------------------------------------
echo "[4/5] 部署 shared/schemas"
USER_DIR="$HOME/Library/Application Support/OpenKey"
SCHEMA_SRC="$REPO_DIR/shared/schemas"
if [ -d "$SCHEMA_SRC" ]; then
  mkdir -p "$USER_DIR/shared"
  # 只覆盖 yaml/json 配置与清单，避免误删用户已部署的 build 产物。
  find "$SCHEMA_SRC" -maxdepth 1 \( -name '*.yaml' -o -name '*.json' \) -exec cp {} "$USER_DIR/shared/" \;
  echo "    已复制 $SCHEMA_SRC/*.yaml,*.json -> $USER_DIR/shared"
  echo "    notice: 词库二进制（*.dict.yaml 编译产物）由 core 首次部署时生成；如方案未出现请删除 $USER_DIR/build 后重试"
else
  echo "notice: 未找到 $SCHEMA_SRC，跳过共享数据部署"
fi

# ---------------------------------------------------------------------------
# 5. 汇总
# ---------------------------------------------------------------------------
echo "[5/5] 完成："
echo "    设置 App : $APP"
echo "    输入法   : $IM"

# ---------------------------------------------------------------------------
# 可选：安装到 /Library/Input Methods（需要管理员权限；安装后需注销重登，
# 见 docs/02 4.2.3）。签名与公证 TODO：
#   TODO(DR-002)：Developer ID Application 证书签名（codesign --options runtime
#     --entitlements ...），再以 notarytool 提交公证；证书经环境变量注入。
#   TODO：App Sandbox + mach-register 例外（连接名 $(PRODUCT_BUNDLE_IDENTIFIER)_Connection）
#     的完整 entitlements 目前未启用（docs/02 4.2.1 注明了安装器生成 entitlements 的方案）。
# ---------------------------------------------------------------------------
if [ "$INSTALL" = "1" ]; then
  echo "[install] 安装到 /Library/Input Methods（需要 sudo）"
  sudo mkdir -p "/Library/Input Methods"
  sudo rm -rf "/Library/Input Methods/OpenKeyInputMethod.inputmethod"
  sudo cp -R "$IM" "/Library/Input Methods/"
  echo "[install] 完成。请到 系统设置 > 键盘 > 输入法 添加 OpenKey（或注销重登后生效）。"
else
  echo "[install] 跳过安装（--install 可安装到 /Library/Input Methods）"
fi
