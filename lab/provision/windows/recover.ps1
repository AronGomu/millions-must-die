# Windows ref external recovery skeleton (T19).
# EXTERNAL CONTROLLER ONLY — never invoke from candidate OS / untrusted tree.
#
# Physical path (blocked until lab exists):
#   1. Power host into WinPE on recovery VLAN
#   2. DISM /Apply-FFU full-drive; controller readback SHA-256
#   3. Mint fresh host identity + unprivileged user; pin power plan
#   4. Move recovery → provisioning → candidate VLANs
#   5. attest.ps1 (host contract)
#   6. External egress canary on candidate VLAN (must DENY)
#   7. Mark ready OR quarantine
#
# This script is a protocol skeleton + dry-run. It does not flash disks.
#Requires -Version 5.1
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Show-Usage {
    @"
Usage:
  recover.ps1 -DryRun [-ImageManifest <path>] [-RunnerManifest <path>]
              [-LabBin <mmd-lab>] [-AttestFixture <json>]
  recover.ps1 -Help

Dry-run validates image-manifest shape + runs protocol simulation via mmd-lab
(windows-recover-simulate). No WinPE, no DISM, no power control.

Live restore requires external controller, RO FFU store, VLANs, Windows 11 ref PC.
Exit: 0 dry-run protocol ready-for-candidate; 1 quarantine/fail; 2 usage/env.
"@
}

$Root = (Resolve-Path (Join-Path $PSScriptRoot '..\..\..')).Path
$ImageManifest = Join-Path $Root 'lab\provision\windows\image-manifest.toml'
$RunnerManifest = Join-Path $Root 'lab\manifests\windows-11-25h2-x86_64.toml'
$AttestFixture = Join-Path $Root 'lab\fixtures\windows-attest\pass.json'
$LabBin = if ($env:MMD_LAB_BIN) { $env:MMD_LAB_BIN } else { '' }
$DryRun = $false
$Help = $false

for ($i = 0; $i -lt $args.Count; $i++) {
    switch -Regex ($args[$i]) {
        '^--dry-run$|^-DryRun$' { $DryRun = $true }
        '^--image-manifest$|^-ImageManifest$' {
            $i++
            if ($i -ge $args.Count) { throw 'missing --image-manifest path' }
            $ImageManifest = [string]$args[$i]
        }
        '^--runner-manifest$|^-RunnerManifest$' {
            $i++
            if ($i -ge $args.Count) { throw 'missing --runner-manifest path' }
            $RunnerManifest = [string]$args[$i]
        }
        '^--attest-fixture$|^-AttestFixture$' {
            $i++
            if ($i -ge $args.Count) { throw 'missing --attest-fixture path' }
            $AttestFixture = [string]$args[$i]
        }
        '^--lab-bin$|^-LabBin$' {
            $i++
            if ($i -ge $args.Count) { throw 'missing --lab-bin path' }
            $LabBin = [string]$args[$i]
        }
        '^--help$|^-Help$|^-h$' { $Help = $true }
        default { throw "unknown arg: $($args[$i])" }
    }
}

if ($Help) {
    Show-Usage
    exit 0
}

if (-not $DryRun) {
    Write-Error @'
live restore not available on this host.
need: Windows 11 25H2 ref PC + WinPE/FFU + external power controller + RO image store + recovery/provisioning/candidate VLANs
use: recover.ps1 --dry-run
'@
    exit 2
}

foreach ($f in @($ImageManifest, $RunnerManifest, $AttestFixture)) {
    if (-not (Test-Path -LiteralPath $f -PathType Leaf)) {
        Write-Error "missing file: $f"
        exit 2
    }
}

$imgText = Get-Content -LiteralPath $ImageManifest -Raw
if ($imgText -notmatch 'initiator\s*=\s*"external-controller"') {
    Write-Error 'image-manifest: initiator must be external-controller'
    exit 1
}
if ($imgText -notmatch 'read_only\s*=\s*true') {
    Write-Error 'image-manifest: ffu must be read_only'
    exit 1
}
if ($imgText -notmatch 'candidate_egress_policy\s*=\s*"deny"') {
    Write-Error 'image-manifest: candidate egress must deny'
    exit 1
}
if ($imgText -notmatch 'boot_env\s*=\s*"winpe"') {
    Write-Error 'image-manifest: boot_env must be winpe'
    exit 1
}
if ($imgText -notmatch 'apply_tool\s*=\s*"dism"') {
    Write-Error 'image-manifest: apply_tool must be dism'
    exit 1
}

if ([string]::IsNullOrWhiteSpace($LabBin)) {
    $cmd = Get-Command mmd-lab -ErrorAction SilentlyContinue
    if ($cmd) {
        $LabBin = $cmd.Source
    } elseif (Test-Path -LiteralPath (Join-Path $env:USERPROFILE '.local\bin\mmd-lab.exe')) {
        $LabBin = Join-Path $env:USERPROFILE '.local\bin\mmd-lab.exe'
    } elseif (Test-Path -LiteralPath (Join-Path $env:HOME '.local/bin/mmd-lab')) {
        $LabBin = Join-Path $env:HOME '.local/bin/mmd-lab'
    } else {
        Write-Error 'mmd-lab not found; set --lab-bin or MMD_LAB_BIN'
        exit 2
    }
}

Write-Host 'recover: dry-run protocol simulation (no disk write)'
Write-Host "recover: image-manifest $ImageManifest"
Write-Host "recover: runner-manifest $RunnerManifest"
Write-Host 'recover: initiator external-controller'
Write-Host 'recover: boot-env winpe'
Write-Host 'recover: apply-tool dism'
Write-Host 'recover: candidate-egress deny'

& $LabBin windows-recover-simulate `
    --image-manifest $ImageManifest `
    --runner-manifest $RunnerManifest `
    --attest-fixture $AttestFixture
exit $LASTEXITCODE
