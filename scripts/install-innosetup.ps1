# Install a pinned Inno Setup into a folder (no admin rights needed) and print the folder.
#
#   .\scripts\install-innosetup.ps1 -Dir C:\tools\InnoSetup
#
# Used by CI and the release workflow so every build uses the same compiler
# version. The installer is checked against a pinned SHA-256 and its
# Authenticode signature. Portable mode: no uninstaller, no Apps entry, and an
# existing Inno Setup installation is left alone. Skips the download when that
# version is already there.
# On GitHub Actions the folder is also added to PATH for later steps.
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Dir
)
$ErrorActionPreference = "Stop"
$version = "6.7.4"
$sha256 = "A549929319F9120716677C685535CA4671E4B937B5FDC87E2585F32D8EB30E8C"
$url = "https://github.com/jrsoftware/issrc/releases/download/is-$($version -replace '\.', '_')/innosetup-$version.exe"

$iscc = Join-Path $Dir "ISCC.exe"
# ISCC.exe carries no usable version resource, so remember what was installed.
$marker = Join-Path $Dir ".installed-version"
$installed = (Test-Path $iscc) -and (Test-Path $marker) -and ((Get-Content $marker -Raw).Trim() -eq $version)
if (-not $installed) {
    $tmp = Join-Path ([IO.Path]::GetTempPath()) "innosetup-$version-$PID.exe"
    $ProgressPreference = "SilentlyContinue"
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    Invoke-WebRequest -Uri $url -OutFile $tmp -UseBasicParsing
    try {
        $hash = (Get-FileHash $tmp -Algorithm SHA256).Hash
        if ($hash -ne $sha256) { throw "Inno Setup download hash mismatch: $hash" }
        if ((Get-AuthenticodeSignature $tmp).Status -ne "Valid") { throw "Inno Setup installer signature is not valid" }
        $p = Start-Process -FilePath $tmp -Wait -PassThru -ArgumentList "/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/SP-", "/CURRENTUSER", "/NOICONS", "/PORTABLE=1", "/DIR=`"$Dir`""
        if ($p.ExitCode -ne 0) { throw "Inno Setup installer failed ($($p.ExitCode))" }
        Set-Content -Path $marker -Value $version
    } finally {
        Remove-Item $tmp -Force -ErrorAction SilentlyContinue
    }
}
if (-not (Test-Path $iscc)) { throw "ISCC.exe not found in $Dir" }
# UTF-8 without BOM (Windows PowerShell's Add-Content would write ANSI).
if ($env:GITHUB_PATH) { [IO.File]::AppendAllText($env:GITHUB_PATH, "$Dir`n") }
Write-Host "Inno Setup $version in $Dir"
