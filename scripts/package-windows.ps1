# 打包 Windows 便携版（S9-04）。
#
# 产物形态：**portable zip**（无安装器开销；需求 §8.1 体积优先于安装体验）。
# 用法：scripts\package-windows.ps1 [-Version 1.0.0]
#   -Version 缺省时取 Cargo.toml 的 package.version。
# 产出：dist\rustmark-v<version>-windows-x64.zip + 同名 .sha256。
#
# 校验：打包后核对 zip 体积 ≤ 11 MiB（安装包口径）并生成 SHA256；
# 纯净虚拟机启动验证属发布演练步骤（docs/release.md），本脚本不做 GUI 冒烟。

param(
    [string]$Version = ""
)

$ErrorActionPreference = "Stop"

# 定位仓库根（脚本在 scripts/ 下）。
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

# 版本号注入：显式参数优先，否则读 Cargo.toml。
if (-not $Version) {
    $Version = (Select-String -Path Cargo.toml -Pattern '^version\s*=\s*"(.+)"' |
        Select-Object -First 1).Matches[0].Groups[1].Value
}
if (-not $Version) { throw "无法确定版本号（Cargo.toml 解析失败）" }
Write-Host "打包版本：v$Version"

# 构建 release（panic=abort / lto 等由 [profile.release] 统一控制）。
cargo build --release --bin rustmark
if ($LASTEXITCODE -ne 0) { throw "cargo build --release 失败" }

# 产物路径随是否显式 target triple 而不同：默认 `target/release/`，
# 配置了默认三元组（本仓库开发机的 .cargo 配置）则为
# `target/<triple>/release/`。两处都找，找不到才报错。
$exe = @(Get-ChildItem -Path target -Recurse -Filter rustmark.exe |
    Where-Object { $_.FullName -match '\\release\\' -and $_.FullName -notmatch '\\deps\\' } |
    Select-Object -First 1).FullName
if (-not $exe) { throw "未找到 rustmark.exe（target/**/release/ 下均无）" }
Write-Host "产物：$exe"

# 组装便携包：exe + 首次运行说明。
$staging = "dist/staging"
if (Test-Path $staging) { Remove-Item -Recurse -Force $staging }
New-Item -ItemType Directory -Path $staging | Out-Null
Copy-Item $exe "$staging/rustmark.exe"
# 内置字体随包分发（体积红线 NFR-PERF-01：字体不进 exe，改为 `fonts/` 目录
# 独立成文件，运行期由 exe 旁的 `fonts/` 读取）。连同许可证一并打包。
Copy-Item -Path "assets/fonts" -Destination "$staging/fonts" -Recurse
# 本项目与第三方依赖的许可文件随包（Apache-2.0 §4.1/§4.2、MIT 第 1 条的
# 再分发要求，见 LEG-01）：与 exe 同级放置，README.txt 指向它们。
Copy-Item "LICENSE-MIT", "LICENSE-APACHE", "NOTICE", "THIRD-PARTY-LICENSES.md" $staging
@"
rustmark v$Version（Windows 便携版）

直接运行 rustmark.exe 即可，无需安装。
设置与草稿保存在 %APPDATA%\rustmark\，删除该目录即完全卸载。

本包未做代码签名：首次运行如遇 SmartScreen 提示，请点「更多信息 → 仍要运行」，
并核对 SHA256 与发布页 checksums 一致。

许可：本软件按 MIT OR Apache-2.0 双许可发布，随包附 LICENSE-MIT / LICENSE-APACHE /
NOTICE（版权与声明）与 THIRD-PARTY-LICENSES.md（第三方依赖清单）；fonts/ 内含字体许可。
"@ | Set-Content -Path "$staging/README.txt" -Encoding UTF8

# 压缩（Compress-Archive 用系统自带的 zip 实现，不引入外部依赖）。
$out = "dist/rustmark-v$Version-windows-x64.zip"
if (Test-Path $out) { Remove-Item -Force $out }
Compress-Archive -Path "$staging/*" -DestinationPath $out
Remove-Item -Recurse -Force $staging

# 体积门禁：红线 11 MiB（安装包口径，与二进制门禁分开计价）。
$packageLimitBytes = 11534336
$size = (Get-Item $out).Length
$mb = [math]::Round($size / 1MB, 3)
Write-Host "产物：$out（$size 字节 ≈ $mb MB）"
if ($size -gt $packageLimitBytes) {
    throw "体积超红线：$size > $packageLimitBytes 字节（11 MiB）"
}

# SHA256 校验和（格式与 sha256sum -c 兼容：<hash>  <filename>）。
$hash = (Get-FileHash $out -Algorithm SHA256).Hash.ToLower()
$line = "$hash  $(Split-Path -Leaf $out)"
Set-Content -Path "$out.sha256" -Value $line -Encoding ASCII
Write-Host "SHA256：$line"
Write-Host "打包完成。"
