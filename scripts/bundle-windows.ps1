# Build OpenVClient for Windows and package a portable zip.
#
#   .\scripts\bundle-windows.ps1                              # x86_64, release build
#   .\scripts\bundle-windows.ps1 -Target aarch64-pc-windows-msvc
#   .\scripts\bundle-windows.ps1 -SkipBuild                   # package an existing build
#
# Signing: if $env:WINDOWS_CERTIFICATE (base64 .pfx) and
# $env:WINDOWS_CERTIFICATE_PASSWORD are set, openvclient.exe is Authenticode-signed
# with signtool before zipping. Otherwise the binary is left unsigned.
#
# Output: dist\OpenVClient-<version>-windows-<arch>-setup.exe (Inno Setup wizard)
#         dist\OpenVClient-<version>-windows-<arch>.zip      (portable)
# The installer needs Inno Setup 6 (winget install JRSoftware.InnoSetup);
# without it only the portable zip is built.
[CmdletBinding()]
param(
    [string]$Target = "x86_64-pc-windows-msvc",
    [switch]$SkipBuild
)
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")

$version = (Select-String -Path Cargo.toml -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
$arch = if ($Target -like "aarch64*") { "aarch64" } else { "x86_64" }

if (-not $SkipBuild) {
    cargo build --release --locked -p openvclient-app --target $Target
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
}
$exe = "target\$Target\release\openvclient.exe"
if (-not (Test-Path $exe)) { throw "binary not found: $exe" }

if ($env:WINDOWS_CERTIFICATE -and $env:WINDOWS_CERTIFICATE_PASSWORD) {
    $pfx = Join-Path $env:TEMP "openvclient-sign.pfx"
    [IO.File]::WriteAllBytes($pfx, [Convert]::FromBase64String($env:WINDOWS_CERTIFICATE))
    $signtool = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\signtool.exe" | Sort-Object FullName -Descending | Select-Object -First 1
    if (-not $signtool) { throw "signtool.exe not found (install the Windows SDK)" }
    & $signtool.FullName sign /f $pfx /p $env:WINDOWS_CERTIFICATE_PASSWORD /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 /d "OpenVClient" $exe
    if ($LASTEXITCODE -ne 0) { throw "signtool failed" }
    Remove-Item $pfx -Force
    Write-Host "Signed $exe"
} else {
    Write-Host "No signing certificate configured; leaving $exe unsigned"
}

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
Write-Host "Built $zip ($([math]::Round((Get-Item $zip).Length / 1MB, 1)) MB)"

# Installer: Inno Setup wizard (Welcome, License, scope, folder, shortcuts, Finish).
$iscc = @(
    (Get-Command iscc.exe -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source),
    "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
    "$env:ProgramFiles\Inno Setup 6\ISCC.exe",
    "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe"
) | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
if ($iscc) {
    $innoArch = if ($arch -eq "aarch64") { "arm64" } else { "x64" }
    $exeFull = (Resolve-Path $exe).Path
    $outDir = (Resolve-Path dist).Path
    & $iscc /Qp "/DAppVersion=$version" "/DSourceExe=$exeFull" "/DArch=$innoArch" "/DOutputDir=$outDir" installer\windows\openvclient.iss
    if ($LASTEXITCODE -ne 0) { throw "Inno Setup failed" }
    $setup = "dist\OpenVClient-$version-windows-$innoArch-setup.exe"
    if ($env:WINDOWS_CERTIFICATE -and $env:WINDOWS_CERTIFICATE_PASSWORD -and $signtool) {
        [IO.File]::WriteAllBytes($pfx, [Convert]::FromBase64String($env:WINDOWS_CERTIFICATE))
        & $signtool.FullName sign /f $pfx /p $env:WINDOWS_CERTIFICATE_PASSWORD /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 /d "OpenVClient Setup" $setup
        Remove-Item $pfx -Force
    }
    Get-FileHash $setup -Algorithm SHA256 | ForEach-Object { "$($_.Hash.ToLower())  $(Split-Path $setup -Leaf)" } | Set-Content "$setup.sha256"
    Write-Host "Built $setup ($([math]::Round((Get-Item $setup).Length / 1MB, 1)) MB)"
} else {
    Write-Warning "Inno Setup 6 not found: skipped the installer. Install it with 'winget install JRSoftware.InnoSetup' and run again."
}

