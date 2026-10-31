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
#
# 公证策略（S9-05）：`NOTARY_APPLE_ID` / `NOTARY_APPLE_PASSWORD` /
# `NOTARY_TEAM_ID` 三者同时配置时，对 .app（经临时 zip）与 .dmg 分别执行
# notarytool 提交 + stapler 装订；任一缺失按配置错误直接失败，不静默降级。
# 公证必须搭配正式证书签名（ad-hoc 签名无法通过公证）。
# 证书与公证凭据绝不入仓，只经 CI secrets 或本机钥匙串注入。

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

# 图标：与 .ico / 窗口图标同源（`assets/logo/build_logo.py` 生成）。
if [ -f assets/logo/rustmark.icns ]; then
    cp assets/logo/rustmark.icns "$app/Contents/Resources/AppIcon.icns"
    /usr/libexec/PlistBuddy -c "Add :CFBundleIconFile string AppIcon" "$app/Contents/Info.plist"
fi

# 内置字体随包分发（体积红线：字体不进二进制）。
# 字体是**资源**，必须放 `Contents/Resources/fonts`：放进 `Contents/MacOS/`
# 会被 codesign 视作代码容器递归检查，对未签名的 .ttf 报
# `code object is not signed at all`，使签名/发布失败。
mkdir -p "$app/Contents/Resources/fonts"
cp assets/fonts/* "$app/Contents/Resources/fonts/"

# 布局断言（fail-closed）：字体在 Resources，且不得残留在 MacOS/。
if [ ! -d "$app/Contents/Resources/fonts" ] || [ -z "$(ls -A "$app/Contents/Resources/fonts")" ]; then
    echo "打包布局错误：Contents/Resources/fonts 缺失或为空" >&2
    exit 1
fi
if [ -e "$app/Contents/MacOS/fonts" ]; then
    echo "打包布局错误：字体不得位于 Contents/MacOS/fonts（会被 codesign 当成代码）" >&2
    exit 1
fi

# 本项目与第三方依赖的许可文件随包（Apache-2.0 §4.1/§4.2、MIT 第 1 条的
# 再分发要求，见 LEG-01）：放 Contents/Resources/（不混入 MacOS/ 签名主体），
# 须在 codesign 之前写入，使许可文件被签名覆盖。
cp LICENSE-MIT LICENSE-APACHE NOTICE THIRD-PARTY-LICENSES.md "$app/Contents/Resources/"

# —— 签名（hardened runtime；无证书时 ad-hoc）——
# 顺序：先签嵌套代码，再签外层 bundle。当前 bundle 内没有 .dylib / .framework /
# 辅助 app，故无需逐个签名；若未来加入，必须在外层之前逐个签。
# **不使用已弃用的 `--deep`**：它把 Contents/MacOS 下的非代码资源也当
# 「子组件」递归检查，是本次 .ttf 报 `code object is not signed at all` 的
# 直接诱因。hardened runtime 与安全时间戳保持开启（时间戳仅正式证书需要）。
identity="${CERT_IDENTITY:-}"
sign_args=(--force --options runtime)
if [ -n "$identity" ]; then
    sign_args+=(--timestamp)
    codesign "${sign_args[@]}" --sign "$identity" "$app"
else
    codesign "${sign_args[@]}" --sign - "$app"
fi

# 签名后必检：`--deep` 此处只用于**验证**（不参与签名），确认整套 bundle 完整。
codesign --verify --deep --strict --verbose=2 "$app"
if [ -n "$identity" ]; then
    echo "（已用证书签名：${identity}）"
else
    # ad-hoc：本机可正常启动；分发场景仍会被 Gatekeeper 拦（未公证），
    # 绕过方式必须写入 README 与 Release Notes。
    echo "（未配置 CERT_IDENTITY，使用 ad-hoc 签名；未公证，需提示用户绕过 Gatekeeper）"
fi

# —— 公证（notarize）+ 装订（staple）——
# 三个变量必须同时配置才启用；任一缺失按**配置错误**失败（fail-closed），
# 避免「以为公证了，其实没提交」。ad-hoc 签名无法公证，故公证必须配证书。
notarize_enabled=0
if [ -n "${NOTARY_APPLE_ID:-}" ] || [ -n "${NOTARY_APPLE_PASSWORD:-}" ] || [ -n "${NOTARY_TEAM_ID:-}" ]; then
    if [ -z "${NOTARY_APPLE_ID:-}" ] || [ -z "${NOTARY_APPLE_PASSWORD:-}" ] || [ -z "${NOTARY_TEAM_ID:-}" ]; then
        echo "公证配置不完整：NOTARY_APPLE_ID / NOTARY_APPLE_PASSWORD / NOTARY_TEAM_ID 必须同时设置" >&2
        exit 1
    fi
    if [ -z "$identity" ]; then
        echo "已配置公证凭据但未提供 CERT_IDENTITY：ad-hoc 签名无法公证" >&2
        exit 1
    fi
    notarize_enabled=1
fi

# 提交公证并等待结果。`notarytool submit --wait` 在 Invalid 时未必以非零退出，
# 因此显式解析 JSON 的 `status`；失败时打印 Apple 的日志便于定位。
notarize() {
    local submit="$1" label="$2" out status ticket
    out="$(mktemp)"
    if ! xcrun notarytool submit "$submit" \
        --apple-id "$NOTARY_APPLE_ID" \
        --team-id "$NOTARY_TEAM_ID" \
        --password "$NOTARY_APPLE_PASSWORD" \
        --wait --output-format json > "$out"; then
        echo "公证提交失败：$label" >&2
        cat "$out" >&2 || true
        rm -f "$out"
        return 1
    fi
    status="$(python3 -c 'import json,sys;print(json.load(open(sys.argv[1])).get("status",""))' "$out")"
    ticket="$(python3 -c 'import json,sys;print(json.load(open(sys.argv[1])).get("id",""))' "$out")"
    printf 'notarytool：%s\n' "$(tr -d '\n' < "$out")"
    rm -f "$out"
    if [ "$status" != "Accepted" ]; then
        echo "公证未通过（${label}）：status=$status id=$ticket" >&2
        if [ -n "$ticket" ]; then
            xcrun notarytool log "$ticket" \
                --apple-id "$NOTARY_APPLE_ID" \
                --team-id "$NOTARY_TEAM_ID" \
                --password "$NOTARY_APPLE_PASSWORD" >&2 || true
        fi
        return 1
    fi
    echo "公证通过：${label}（id=${ticket}）"
}

if [ "$notarize_enabled" -eq 1 ]; then
    # 公证只接受 zip / dmg / pkg：用 ditto 打包 app（临时目录，不污染 dist）。
    submit_dir="$(mktemp -d)"
    ditto -c -k --keepParent "$app" "$submit_dir/Rustmark.zip"
    notarize "$submit_dir/Rustmark.zip" "app bundle"
    rm -rf "$submit_dir"
    # 装订票根进 .app：解压 zip 后离线首次打开也无需联网校验。
    xcrun stapler staple "$app"
    xcrun stapler validate "$app"
fi

# —— zip（保符号链接与权限，zip -y；须在 staple 之后，票根随包分发）——
mkdir -p dist
appzip="dist/rustmark-v$version-macos-$arch.zip"
rm -f "$appzip"
(cd dist && zip -qryy "$(basename "$appzip")" "$(basename "$app")")

# —— .dmg ——
dmg="dist/rustmark-v$version-macos.dmg"
rm -f "$dmg"
hdiutil create -volname "Rustmark $version" -srcfolder "$app" -ov -format UDZO "$dmg"

if [ "$notarize_enabled" -eq 1 ]; then
    # dmg 单独公证并装订：用户直接打开 dmg 时 Gatekeeper 评估的是 dmg 本身。
    notarize "$dmg" "dmg"
    xcrun stapler staple "$dmg"
    xcrun stapler validate "$dmg"
fi

# —— 体积门禁 + 校验和（安装包口径，与二进制门禁分开计价）——
packageLimitBytes=11534336
for f in "$appzip" "$dmg"; do
    size=$(wc -c < "$f" | tr -d ' ')
    # 变量后紧跟全角标点须用 ${}：macOS bash 3.2 会把多字节字符并入变量名。
    echo "产物：${f}（${size} 字节）"
    if [ "$size" -gt "$packageLimitBytes" ]; then
        echo "体积超红线：$size > $packageLimitBytes 字节（11 MiB）" >&2
        exit 1
    fi
done
for f in "$appzip" "$dmg"; do
    shasum -a 256 "$f" > "$f.sha256"
    cat "$f.sha256"
done

echo "打包完成。"
