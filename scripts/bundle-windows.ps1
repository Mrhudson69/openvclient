# Build OpenVClient for Windows: an Inno Setup installer and a portable zip.
#
#   .\scripts\bundle-windows.ps1                              # x86_64, release build
#   .\scripts\bundle-windows.ps1 -Target aarch64-pc-windows-msvc
#   .\scripts\bundle-windows.ps1 -SkipBuild                   # package an existing build
#
# Works in Windows PowerShell 5.1 and PowerShell 7.
#
# Needs: Rust (MSVC toolchain), Visual Studio Build Tools with the Windows SDK
# (rc.exe embeds the icon), and Inno Setup 6.6 or newer for the installer
# (winget install JRSoftware.InnoSetup). Without Inno Setup only the zip is built.
#
# Signing: if $env:WINDOWS_CERTIFICATE (base64 .pfx) and
# $env:WINDOWS_CERTIFICATE_PASSWORD are set, OpenVClient.exe and the installer
# are Authenticode-signed with signtool. Otherwise both are left unsigned.
#
# Output: dist\OpenVClient-<version>-windows-<x64|arm64>-setup.exe (+ .sha256)
#         dist\OpenVClient-<version>-windows-<x86_64|aarch64>.zip  (+ .sha256)
[CmdletBinding()]
param(
    [string]$Target = "x86_64-pc-windows-msvc",
    [switch]$SkipBuild
)
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")
$root = (Get-Location).Path

# rustup installs to ~\.cargo\bin; a fresh shell may not have it on PATH yet.
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    $cargoBin = Join-Path $env:USERPROFILE ".cargo\bin"
    if (Test-Path (Join-Path $cargoBin "cargo.exe")) { $env:PATH = "$cargoBin;$env:PATH" }
}

# [workspace.package] version in the root Cargo.toml is the single source of truth.
$version = (Select-String -Path Cargo.toml -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
$arch = if ($Target -like "aarch64*") { "aarch64" } else { "x86_64" }
$innoArch = if ($arch -eq "aarch64") { "arm64" } else { "x64" }
Write-Host "OpenVClient $version for $Target"

function Write-Sha256([string]$Path) {
    $h = (Get-FileHash $Path -Algorithm SHA256).Hash.ToLower()
    # UTF-8 without BOM, LF, the format `sha256sum -c` reads.
    [IO.File]::WriteAllText("$Path.sha256", "$h  $(Split-Path $Path -Leaf)`n")
}

$signtool = $null
$pfx = Join-Path ([IO.Path]::GetTempPath()) "openvclient-sign.pfx"
$signing = [bool]($env:WINDOWS_CERTIFICATE -and $env:WINDOWS_CERTIFICATE_PASSWORD)
if ($signing) {
    $signtool = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\signtool.exe" -ErrorAction SilentlyContinue | Sort-Object FullName -Descending | Select-Object -First 1
    if (-not $signtool) { throw "signtool.exe not found (install the Windows SDK)" }
}
function Invoke-Sign([string]$File, [string]$Description) {
    if (-not $signing) { return }
    [IO.File]::WriteAllBytes($pfx, [Convert]::FromBase64String($env:WINDOWS_CERTIFICATE))
    try {
        & $signtool.FullName sign /f $pfx /p $env:WINDOWS_CERTIFICATE_PASSWORD /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 /d $Description $File
        if ($LASTEXITCODE -ne 0) { throw "signtool failed for $File" }
    } finally {
        Remove-Item $pfx -Force -ErrorAction SilentlyContinue
    }
    Write-Host "Signed $File"
}

if (-not $SkipBuild) {
    # Fail instead of shipping an exe without its icon and version resource.
    $env:OPENVCLIENT_REQUIRE_RESOURCES = "1"
    cargo build --release --locked -p openvclient-app --target $Target
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
}
$targetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { "target" }
$exe = Join-Path $targetDir "$Target\release\openvclient.exe"
if (-not (Test-Path $exe)) { throw "binary not found: $exe" }
$exeVersion = (Get-Item $exe).VersionInfo
if (-not $exeVersion.ProductName) { throw "$exe has no version resource (rc.exe missing?). Install the Windows SDK and rebuild." }

if ($signing) { Invoke-Sign $exe "OpenVClient" } else { Write-Host "No signing certificate configured; leaving the binaries unsigned" }

# Portable zip.
$name = "OpenVClient-$version-windows-$arch"
$stage = Join-Path ([IO.Path]::GetTempPath()) $name
if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
New-Item -ItemType Directory -Path $stage | Out-Null
Copy-Item $exe (Join-Path $stage "OpenVClient.exe")
foreach ($f in "README.md", "LICENSE-MIT", "LICENSE-APACHE") {
    if (Test-Path $f) { Copy-Item $f $stage }
}
New-Item -ItemType Directory -Path dist -Force | Out-Null
$zip = "dist\$name.zip"
if (Test-Path $zip) { Remove-Item $zip -Force }
Compress-Archive -Path (Join-Path $stage "*") -DestinationPath $zip
Remove-Item $stage -Recurse -Force
Write-Sha256 $zip
Write-Host "Built $zip ($([math]::Round((Get-Item $zip).Length / 1MB, 1)) MB)"

# Installer: Inno Setup wizard (scope, Welcome, License, folder, Start menu, shortcuts, Finish).
$iscc = @(
    (Get-Command iscc.exe -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source),
    "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
    "$env:ProgramFiles\Inno Setup 6\ISCC.exe",
    "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe"
) | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
if (-not $iscc) {
    Write-Warning "Inno Setup 6 not found: skipped the installer. Install it with 'winget install JRSoftware.InnoSetup' and run again."
    return
}

# License page: the project is MIT OR Apache-2.0, so show both texts.
# Combine, not Join-Path: CARGO_TARGET_DIR may be absolute.
$work = Join-Path ([IO.Path]::GetFullPath([IO.Path]::Combine($root, $targetDir))) "installer"
New-Item -ItemType Directory -Path $work -Force | Out-Null
$license = Join-Path $work "license.txt"
$intro = "OpenVClient is licensed under either of the MIT License or the Apache License, Version 2.0, at your option. Both texts follow and are installed next to the program.`r`n`r`n"
$rule = "`r`n`r`n" + ("=" * 72) + "`r`n`r`n"
$body = $intro + (Get-Content LICENSE-MIT -Raw -Encoding UTF8) + $rule + (Get-Content LICENSE-APACHE -Raw -Encoding UTF8)
[IO.File]::WriteAllText($license, ($body -replace "(?<!`r)`n", "`r`n"), (New-Object Text.UTF8Encoding $true))

$exeFull = (Resolve-Path $exe).Path
$outDir = (Resolve-Path dist).Path
& $iscc /Qp "/DAppVersion=$version" "/DSourceExe=$exeFull" "/DArch=$innoArch" "/DOutputDir=$outDir" "/DLicenseText=$license" installer\windows\openvclient.iss
if ($LASTEXITCODE -ne 0) { throw "Inno Setup failed (it needs version 6.6 or newer)" }
$setup = "dist\OpenVClient-$version-windows-$innoArch-setup.exe"
Invoke-Sign $setup "OpenVClient Setup"
Write-Sha256 $setup
Write-Host "Built $setup ($([math]::Round((Get-Item $setup).Length / 1MB, 1)) MB)"
