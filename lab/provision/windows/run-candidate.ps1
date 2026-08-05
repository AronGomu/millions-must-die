# Windows candidate gate lane (T20).
# EXTERNAL COORDINATOR ONLY — never invoke from candidate OS / untrusted tree.
#
# Lane (coordinator-verified; candidate report never trusted):
#   1. Coordinator self-check
#   2. Host attestation (identity only; Basic Render Driver rejected)
#      + recovery ready-for-candidate gate
#   3. Deliver exact content-addressed archive; remote hash verify (mismatch = no exec)
#   4. Run visible D3D12 smoke + offscreen scale curve   [deferred-hw: native run]
#   5. Collect raw samples / readback / manifests
#   6. Coordinator recomputes stats + golden diff from raw evidence
#   7. Force post-run external WinPE/FFU restore          [deferred-hw: physical reset]
#
# Fixture mode drives the exact coordinator protocol with fake transport +
# committed raw/report fixtures. Live mode needs the physical Windows ref lab.
#
# Bounded risk (accepted): attestation covers host/source identity only; a
# hostile candidate can still deny service or forge its raw output wholesale —
# including the adapter/os strings the golden binding reads from candidate
# evidence. Coordinator recompute + forced reset bound, but do not remove,
# that risk.
#
# Fixture note: the committed pass fixtures bind to a synthetic fixture golden
# (lab/fixtures/windows-candidate/golden). The reviewed windows-d3d12 golden
# stays an honest deferred-hw placeholder until native capture on the frozen
# RX 6400 ref host retires this binding.
#Requires -Version 5.1
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Show-Usage {
    @"
Usage:
  run-candidate.ps1 --fixture-mode --commit <exact-hash> [--lab-bin <mmd-lab>]
                    [--evidence <json>] [--readback <png>] [--attest <json>]
                    [--runner-manifest <toml>] [--golden-dir <dir>] [--root <dir>]
                    [--skip-self-check]
  run-candidate.ps1 --help

Fixture mode runs the trusted coordinator lane (`mmd-lab validate-runner
--runner windows`) against fake transport + raw/report fixtures.

Live candidate execution requires the physical Windows ref host + external
WinPE/FFU recovery lab (deferred-hw).

Exit: 0 coordinator-verified pass; 1 fail; 2 usage/env/inconclusive;
      3 coordinator error.
"@
}

$Root = (Resolve-Path (Join-Path $PSScriptRoot '..\..\..')).Path
$LabBin = if ($env:MMD_LAB_BIN) { $env:MMD_LAB_BIN } else { '' }
$FixtureMode = $false
$SkipSelfCheck = $false
$Commit = ''
$Evidence = ''
$Readback = ''
$Attest = ''
$RunnerManifest = ''
$GoldenDir = ''
$SrcRoot = ''
$Help = $false

function Get-ArgValue {
    param([object[]]$ArgList, [int]$Index, [string]$Name)
    if ($Index -ge $ArgList.Count) {
        [Console]::Error.WriteLine("missing value for $Name")
        exit 2
    }
    return [string]$ArgList[$Index]
}

for ($i = 0; $i -lt $args.Count; $i++) {
    switch -Regex ($args[$i]) {
        '^--fixture-mode$|^-FixtureMode$' { $FixtureMode = $true }
        '^--skip-self-check$|^-SkipSelfCheck$' { $SkipSelfCheck = $true }
        '^--commit$|^-Commit$' { $i++; $Commit = Get-ArgValue $args $i '--commit' }
        '^--evidence$|^-Evidence$' { $i++; $Evidence = Get-ArgValue $args $i '--evidence' }
        '^--readback$|^-Readback$' { $i++; $Readback = Get-ArgValue $args $i '--readback' }
        '^--attest$|^-Attest$' { $i++; $Attest = Get-ArgValue $args $i '--attest' }
        '^--runner-manifest$|^-RunnerManifest$' { $i++; $RunnerManifest = Get-ArgValue $args $i '--runner-manifest' }
        '^--golden-dir$|^-GoldenDir$' { $i++; $GoldenDir = Get-ArgValue $args $i '--golden-dir' }
        '^--root$|^-Root$' { $i++; $SrcRoot = Get-ArgValue $args $i '--root' }
        '^--lab-bin$|^-LabBin$' { $i++; $LabBin = Get-ArgValue $args $i '--lab-bin' }
        '^--help$|^-Help$|^-h$' { $Help = $true }
        default { [Console]::Error.WriteLine("unknown arg: $($args[$i])"); exit 2 }
    }
}

if ($Help) {
    Show-Usage
    exit 0
}

if (-not $FixtureMode) {
    [Console]::Error.WriteLine(@'
live candidate run not available on this host (deferred-hw).
need: Windows 11 25H2 ref PC (8600G / RX 6400) + external WinPE/FFU recovery lab (T19)
use: run-candidate.ps1 --fixture-mode --commit <exact-hash>
'@)
    exit 2
}

if ([string]::IsNullOrWhiteSpace($Commit)) {
    [Console]::Error.WriteLine('missing --commit <exact-hash> (candidate identity is mandatory)')
    exit 2
}

if ([string]::IsNullOrWhiteSpace($LabBin)) {
    $cmd = Get-Command mmd-lab -ErrorAction SilentlyContinue
    if ($cmd) {
        $LabBin = $cmd.Source
    } elseif ($env:USERPROFILE -and (Test-Path -LiteralPath (Join-Path $env:USERPROFILE '.local\bin\mmd-lab.exe'))) {
        $LabBin = Join-Path $env:USERPROFILE '.local\bin\mmd-lab.exe'
    } elseif ($env:HOME -and (Test-Path -LiteralPath (Join-Path $env:HOME '.local/bin/mmd-lab'))) {
        $LabBin = Join-Path $env:HOME '.local/bin/mmd-lab'
    } else {
        [Console]::Error.WriteLine('mmd-lab not found; set --lab-bin or MMD_LAB_BIN')
        exit 2
    }
}

$LaneArgs = @('validate-runner', '--runner', 'windows', '--commit', $Commit)
if ($SkipSelfCheck) { $LaneArgs += '--skip-self-check' }
if (-not [string]::IsNullOrWhiteSpace($SrcRoot)) { $LaneArgs += @('--root', $SrcRoot) }
if (-not [string]::IsNullOrWhiteSpace($RunnerManifest)) { $LaneArgs += @('--runner-manifest', $RunnerManifest) }
if (-not [string]::IsNullOrWhiteSpace($Attest)) { $LaneArgs += @('--attest-fixture', $Attest) }
if (-not [string]::IsNullOrWhiteSpace($Evidence)) { $LaneArgs += @('--evidence-fixture', $Evidence) }
if (-not [string]::IsNullOrWhiteSpace($Readback)) { $LaneArgs += @('--readback-fixture', $Readback) }
if (-not [string]::IsNullOrWhiteSpace($GoldenDir)) { $LaneArgs += @('--golden-dir', $GoldenDir) }

Write-Output 'run-candidate: fixture mode (fake transport; native D3D12 run deferred-hw)'
Write-Output "run-candidate: coordinator $LabBin"
Write-Output "run-candidate: workspace $Root"

& $LabBin @LaneArgs
exit $LASTEXITCODE
