#!/usr/bin/env bash
# 打包 macOS .app bundle + .dmg（S9-04）。
#
# 用法：scripts/package-macos.sh [版本号]
#   版本号缺省时取 Cargo.toml 的 package.version。
# 产出：dist/rustmark-v<version>-macos-arm64.zip（.app）与
#       dist/rustmark-v<version>-macos.dmg、对应 .sha256。
#
# 签名策略（S9-05）：有 CERT_IDENTITY 环境变量时做正式 codesign，
# 否则 **ad-hoc 签名**（--sign -，保证 hardened runtime 元数据一致），
# 并在 README/发布页明确「未公证，首次打开需右键 → 打开」。
# 证书绝不入仓，只经 CI secrets 或本机钥匙串注入。

set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

# 版本号注入。
version="${1:-}"
if [ -z "$version" ]; then
    version="$(sed -n 's/^version[[:space:]]*=[[:space:]]*"\(.*\)"/\1/p' Cargo.toml | head -1)"
fi
[ -n "$version" ] || { echo "无法确定版本号" >&2; exit 1; }
echo "打包版本：v$version"

arch="$(uname -m)"
cargo build --release --bin rustmark
# 产物路径随是否显式 target triple 而不同：默认 `target/release/`，
# 配置了默认三元组的机器为 `target/<triple>/release/`。两处都找。
exe="$(find target -type f -name rustmark -path '*release*' -not -path '*/deps/*' | head -1)"
[ -n "$exe" ] || { echo "未找到 rustmark（target/**/release/ 下均无）" >&2; exit 1; }
echo "产物：$exe"

# —— .app bundle ——
app="dist/Rustmark.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$exe" "$app/Contents/MacOS/rustmark"

cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key><string>rustmark</string>
    <key>CFBundleIdentifier</key><string>io.github.rustmark.Rustmark</string>
    <key>CFBundleName</key><string>Rustmark</string>
    <key>CFBundleDisplayName</key><string>Rustmark</string>
    <key>CFBundleVersion</key><string>$version</string>
    <key>CFBundleShortVersionString</key><string>$version</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>LSMinimumSystemVersion</key><string>10.15</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>CFBundleDocumentTypes</key>
    <array>
        <dict>
            <key>CFBundleTypeName</key><string>Markdown Document</string>
            <key>CFBundleTypeExtensions</key><array><string>md</string><string>markdown</string></array>
            <key>CFBundleTypeRole</key><string>Editor</string>
        </dict>
    </array>
</dict>
</plist>
PLIST

# 图标可选：仓库提供 assets/icon.icns 时才写入（缺省用系统默认图标）。
if [ -f assets/icon.icns ]; then
    cp assets/icon.icns "$app/Contents/Resources/AppIcon.icns"
    /usr/libexec/PlistBuddy -c "Add :CFBundleIconFile string AppIcon" "$app/Contents/Info.plist"
fi

# 内置字体随包分发（体积红线：字体不进二进制，改为 exe 旁的 `fonts/` 目录，
# 运行期按 `current_exe` 定位读取）。连同许可证一并打入 bundle（须在签名前，
# 使字体文件被 codesign 覆盖）。
mkdir -p "$app/Contents/MacOS/fonts"
cp assets/fonts/* "$app/Contents/MacOS/fonts/"

# 本项目与第三方依赖的许可文件随包（Apache-2.0 §4.1/§4.2、MIT 第 1 条的
# 再分发要求，见 LEG-01）：放 Contents/Resources/（不混入 MacOS/ 签名主体），
# 须在 codesign 之前写入，使许可文件被签名覆盖。
cp LICENSE-MIT LICENSE-APACHE NOTICE THIRD-PARTY-LICENSES.md "$app/Contents/Resources/"

# —— 签名（hardened runtime；无证书时 ad-hoc）——
identity="${CERT_IDENTITY:-}"
if [ -n "$identity" ]; then
    codesign --force --options runtime --timestamp \
        --sign "$identity" "$app"
    codesign --verify --strict "$app"
else
    # ad-hoc：本机可正常启动；分发场景仍会被 Gatekeeper 拦（未公证），
    # 绕过方式必须写入 README 与 Release Notes。
    codesign --force --options runtime --sign - "$app"
    echo "（未配置 CERT_IDENTITY，使用 ad-hoc 签名；未公证，需提示用户绕过 Gatekeeper）"
fi

# —— zip（保符号链接与权限，zip -y）——
mkdir -p dist
appzip="dist/rustmark-v$version-macos-$arch.zip"
rm -f "$appzip"
(cd dist && zip -qryy "$(basename "$appzip")" "$(basename "$app")")

# —— .dmg ——
dmg="dist/rustmark-v$version-macos.dmg"
rm -f "$dmg"
hdiutil create -volname "Rustmark $version" -srcfolder "$app" -ov -format UDZO "$dmg"

# —— 体积门禁 + 校验和 ——
limit=10485760
for f in "$appzip" "$dmg"; do
    size=$(wc -c < "$f" | tr -d ' ')
    echo "产物：$f（$size 字节）"
    if [ "$size" -gt "$limit" ]; then
        echo "体积超红线：$size > $limit 字节（10 MB）" >&2
        exit 1
    fi
done
for f in "$appzip" "$dmg"; do
    shasum -a 256 "$f" > "$f.sha256"
    cat "$f.sha256"
done

echo "打包完成。"
