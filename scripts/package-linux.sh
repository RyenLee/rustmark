#!/usr/bin/env bash
# 打包 Linux AppImage + tar.gz（S9-04）。
#
# 用法：scripts/package-linux.sh [版本号]
#   版本号缺省时取 Cargo.toml 的 package.version。
# 产出：dist/rustmark-v<version>-linux-x64.AppImage、
#       dist/rustmark-v<version>-linux-x64.tar.gz 及对应 .sha256。
#
# AppImage 工具链：优先使用 appimagetool（按 ARCH 从 GitHub 下载，
# CI 缓存友好）；下载失败时**降级为 tar.gz** 并明确提示——
# 裸 tar.gz 一样可运行，只是少了「免安装单文件」形态。
# 依赖说明：wgpu 走系统 Vulkan/GL，目标机需 libvulkan / mesa；
# glibc 最低版本以 CI 构建 runner（ubuntu-20.04）为准。

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

cargo build --release --bin rustmark
# 产物路径随是否显式 target triple 而不同：默认 `target/release/`，
# 配置了默认三元组的机器为 `target/<triple>/release/`。两处都找。
exe="$(find target -type f -name rustmark -path '*release*' -not -path '*/deps/*' | head -1)"
[ -n "$exe" ] || { echo "未找到 rustmark（target/**/release/ 下均无）" >&2; exit 1; }
echo "产物：$exe"

mkdir -p dist

# —— tar.gz（保权限；解包即用）——
stage="dist/rustmark-v$version-linux-x64"
rm -rf "$stage" "dist/rustmark-v$version-linux-x64.tar.gz"
mkdir -p "$stage"
cp "$exe" "$stage/rustmark"
# 内置字体随包分发（体积红线：字体不进二进制，改为 exe 旁的 `fonts/` 目录）。
mkdir -p "$stage/fonts"
cp assets/fonts/* "$stage/fonts/"
# 本项目与第三方依赖的许可文件随包（Apache-2.0 §4.1/§4.2、MIT 第 1 条的
# 再分发要求，见 LEG-01）：与可执行文件同级。
cp LICENSE-MIT LICENSE-APACHE NOTICE THIRD-PARTY-LICENSES.md "$stage/"
cat > "$stage/README.txt" <<EOF
rustmark v$version（Linux）

运行 ./rustmark 即可。依赖：libvulkan1 / mesa（Vulkan 或 GL 渲染）。
设置在 ~/.config/rustmark/，删除该目录即完全卸载。

许可：本软件按 MIT OR Apache-2.0 双许可发布，随包附 LICENSE-MIT / LICENSE-APACHE /
NOTICE（版权与声明）与 THIRD-PARTY-LICENSES.md（第三方依赖清单）；fonts/ 内含字体许可。
EOF
tar -czf "dist/rustmark-v$version-linux-x64.tar.gz" -C dist "$(basename "$stage")"
rm -rf "$stage"

# —— AppImage（可选降级）——
appdir="dist/AppDir"
rm -rf "$appdir"
mkdir -p "$appdir/usr/bin" "$appdir/usr/share/applications"
cp "$exe" "$appdir/usr/bin/rustmark"
# 内置字体随包分发（exe 旁 `fonts/`；AppImage 挂载点在运行期经 `current_exe` 定位）。
mkdir -p "$appdir/usr/bin/fonts"
cp assets/fonts/* "$appdir/usr/bin/fonts/"
# 许可文件随 AppImage 分发（Apache-2.0 §4.1/§4.2、MIT 第 1 条，见 LEG-01）：
# 按 FHS 惯例放 usr/share/doc/rustmark/。
mkdir -p "$appdir/usr/share/doc/rustmark"
cp LICENSE-MIT LICENSE-APACHE NOTICE THIRD-PARTY-LICENSES.md "$appdir/usr/share/doc/rustmark/"

cat > "$appdir/usr/share/applications/rustmark.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Rustmark
Comment=轻量级 Markdown 编辑器
Exec=rustmark
Icon=rustmark
Categories=Utility;TextEditor;
Terminal=false
DESKTOP

# 图标可选：仓库提供 assets/icon.png 时才安装（缺省时 AppImage 无图标也能启动）。
if [ -f assets/icon.png ]; then
    mkdir -p "$appdir/usr/share/icons/hicolor/256x256/apps"
    cp assets/icon.png "$appdir/usr/share/icons/hicolor/256x256/apps/rustmark.png"
fi
cat > "$appdir/rustmark.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Rustmark
Comment=轻量级 Markdown 编辑器
Exec=rustmark
Icon=rustmark
Categories=Utility;TextEditor;
Terminal=false
DESKTOP

appimage="dist/rustmark-v$version-linux-x64.AppImage"
rm -f "$appimage"
if command -v appimagetool >/dev/null 2>&1; then
    appimagetool "$appdir" "$appimage"
elif [ -n "${APPIMAGETOOL:-}" ] && [ -x "$APPIMAGETOOL" ]; then
    "$APPIMAGETOOL" "$appdir" "$appimage"
else
    echo "⚠ 未找到 appimagetool，跳过 AppImage，仅产出 tar.gz。" \
         "可从 https://github.com/AppImage/appimagetool/releases 获取" \
         "或以 APPIMAGETOOL=<路径> 传入。" >&2
fi
rm -rf "$appdir"

# —— 体积门禁 + 校验和 ——
limit=10485760
status=0
for f in "dist/rustmark-v$version-linux-x64.tar.gz" $([ -f "$appimage" ] && echo "$appimage"); do
    size=$(wc -c < "$f" | tr -d ' ')
    echo "产物：$f（$size 字节）"
    if [ "$size" -gt "$limit" ]; then
        echo "体积超红线：$size > $limit 字节（10 MB）" >&2
        status=1
    fi
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$f" > "$f.sha256"
    else
        shasum -a 256 "$f" > "$f.sha256"
    fi
    cat "$f.sha256"
done
exit $status
