# Writes the notes of a GitHub release: how to install, the SHA-256 checksums,
# the CHANGELOG section of the version and the unofficial-client warning.
# Used by .github/workflows/release.yml; can be run locally to preview the notes:
#   powershell -ExecutionPolicy Bypass -File scripts\release-notes.ps1 -Version 1.0.0 -Tag v1.0.0 -Sums dist\SHA256SUMS.txt -Out dist\RELEASE_NOTES.md
# It fails when CHANGELOG.md has no "## [<version>]" section, so a release never
# goes out without notes (rename "[Unreleased]" in the release pull request).

param(
    [Parameter(Mandatory)][string]$Version,
    [Parameter(Mandatory)][string]$Tag,
    [Parameter(Mandatory)][string]$Sums,
    [Parameter(Mandatory)][string]$Out
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot

$changelog = Get-Content -LiteralPath (Join-Path $root "CHANGELOG.md") -Raw
$heading = [regex]::Match($changelog, "(?m)^## \[$([regex]::Escape($Version))\][^\r\n]*\r?\n")
if (-not $heading.Success) { throw "CHANGELOG.md has no '## [$Version]' section" }
$rest = $changelog.Substring($heading.Index + $heading.Length)
$next = [regex]::Match($rest, "(?m)^## \[")
$section = $(if ($next.Success) { $rest.Substring(0, $next.Index) } else { $rest }).Trim()
if (-not $section) { throw "The CHANGELOG.md section for $Version is empty" }

$checksums = (Get-Content -LiteralPath $Sums -Raw).Replace("`r", "").Trim()
if (-not $checksums) { throw "$Sums is empty" }

$setup = "Abyss-$Version-win-x64-setup.exe"
$notes = @"
## Install

Download **$setup**, check it (below) and run it. It installs for your user only and needs no administrator rights. A portable ``.zip`` is also attached.

Windows SmartScreen may say "Windows protected your PC": the files are **not code-signed**. If you downloaded them from this page and the checksum matches, choose **More info**, then **Run anyway**.

## Checksums (SHA-256)

``````text
$checksums
``````

To check a file, run this in PowerShell in the folder with the download and compare the result with the list above:

``````powershell
Get-FileHash -Algorithm SHA256 .\$setup
``````

The installer and the zip also carry a build provenance attestation from GitHub. With the GitHub CLI: ``gh attestation verify .\$setup --repo RaunaCO/Abyss``

## What changed

$section

## Before you sign in with TIDAL

Abyss is an independent project. It is not made, supported or approved by TIDAL, and it signs in using identifiers that belong to TIDAL's own apps. TIDAL could suspend your account or block Abyss. Use it at your own risk. Playing only your own files never uses a TIDAL account. See [docs/LEGAL.md](https://github.com/RaunaCO/Abyss/blob/$Tag/docs/LEGAL.md).
"@

$target = $(if ([IO.Path]::IsPathRooted($Out)) { $Out } else { Join-Path (Get-Location).Path $Out })
[IO.File]::WriteAllText($target, $notes, (New-Object Text.UTF8Encoding($false)))
Write-Host "Wrote $Out"
