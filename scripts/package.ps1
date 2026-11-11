# Builds Abyss in release mode and assembles the Windows x64 portable package:
#   dist\Abyss-<version>-win-x64\        (folder)
#   dist\Abyss-<version>-win-x64.zip     (portable archive)
#   dist\SHA256SUMS.txt                 (checksums of the zip and the exe)
# Usage (from the repository root):  powershell -ExecutionPolicy Bypass -File scripts\package.ps1
# The build uses its own target directory so a running Abyss does not block it.
# Binaries are NOT code-signed (no certificate).

param([switch]$Installer)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
$name = "Abyss-$version-win-x64"
$targetDir = Join-Path $root "target\package"

Write-Host "Building Abyss $version (release)..."
cargo build --release --locked --target-dir $targetDir
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

$dist = Join-Path $root "dist"
if ((Test-Path -LiteralPath $dist) -and ((Get-Item -LiteralPath $dist).Attributes -band [IO.FileAttributes]::ReparsePoint)) {
    throw "Refusing to package into a linked dist directory"
}
$out = Join-Path $dist $name
$resolvedOut = [IO.Path]::GetFullPath($out)
$distBoundary = [IO.Path]::GetFullPath($dist) + [IO.Path]::DirectorySeparatorChar
if (-not $resolvedOut.StartsWith($distBoundary, [StringComparison]::OrdinalIgnoreCase)) {
    throw "Package output must stay inside the repository dist directory"
}
if (Test-Path -LiteralPath $resolvedOut) {
    if ((Get-Item -LiteralPath $resolvedOut).Attributes -band [IO.FileAttributes]::ReparsePoint) {
        throw "Refusing to replace a linked package directory"
    }
    Remove-Item -LiteralPath $resolvedOut -Recurse -Force
}
New-Item -ItemType Directory -Force (Join-Path $out "licenses") | Out-Null

Copy-Item (Join-Path $targetDir "release\Abyss.exe") (Join-Path $out "Abyss.exe")
Copy-Item README.md, LICENSE, CHANGELOG.md $out
Copy-Item docs\LEGAL.md (Join-Path $out "LEGAL.md")
Copy-Item docs\PRIVACY.md (Join-Path $out "PRIVACY.md")
Copy-Item THIRD_PARTY_NOTICES.md (Join-Path $out "THIRD_PARTY_NOTICES.md")
Copy-Item docs\CONFIGURATION.md (Join-Path $out "CONFIGURATION.md")
Copy-Item assets\fonts\Inter-LICENSE.txt (Join-Path $out "licenses\Inter-OFL.txt")
Copy-Item assets\licenses\IPADIC-COPYING.txt (Join-Path $out "licenses\IPADIC-COPYING.txt")
Copy-Item docs\DEPENDENCIES.md (Join-Path $out "licenses\DEPENDENCIES.md")

$zip = Join-Path $dist "$name.zip"
if (Test-Path $zip) { Remove-Item $zip -Force }
Compress-Archive -Path $out -DestinationPath $zip

$lines = foreach ($f in @($zip, (Join-Path $out "Abyss.exe"))) {
    $h = (Get-FileHash -Algorithm SHA256 $f).Hash.ToLower()
    "$h  $([IO.Path]::GetFileName($f))"
}
$lines | Set-Content -Encoding ascii (Join-Path $dist "SHA256SUMS.txt")
$lines
Write-Host "Done: $zip"

if ($Installer) {
    $compiler = Get-Command iscc.exe -ErrorAction Stop
    & $compiler.Source "/DVersion=$version" scripts\abyss.iss
    if ($LASTEXITCODE -ne 0) { throw "Inno Setup failed" }
    $setup = Join-Path $dist "$name-setup.exe"
    $hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $setup).Hash.ToLower()
    "$hash  $([IO.Path]::GetFileName($setup))" | Add-Content -Encoding ascii (Join-Path $dist "SHA256SUMS.txt")
}
