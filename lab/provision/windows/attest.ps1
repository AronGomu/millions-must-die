# Windows ref host inspect + contract check (dry-run via fixtures; live inspect later).
# Host attestation only — never candidate evidence / trial samples.
#Requires -Version 5.1
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Show-Usage {
    @"
Usage:
  attest.ps1 -DryRun -Fixture <path.json> [-Manifest <path.toml>] [-LabBin <mmd-lab>]
  attest.ps1 -Help

Dry-run loads observed attestation JSON fixture and validates against frozen
Windows runner manifest via trusted mmd-lab (or cargo bin under test).

Exit: 0 ready-for-recovery; 1 quarantine|reject|maintenance-block; 2 usage/tool error.
"@
}

$Root = (Resolve-Path (Join-Path $PSScriptRoot '..\..\..')).Path
$Manifest = Join-Path $Root 'lab\manifests\windows-11-25h2-x86_64.toml'
$LabBin = if ($env:MMD_LAB_BIN) { $env:MMD_LAB_BIN } else { '' }
$DryRun = $false
$Fixture = ''
$Help = $false

for ($i = 0; $i -lt $args.Count; $i++) {
    switch -Regex ($args[$i]) {
        '^--dry-run$|^-DryRun$' { $DryRun = $true }
        '^--fixture$|^-Fixture$' {
            $i++
            if ($i -ge $args.Count) { throw 'missing --fixture path' }
            $Fixture = [string]$args[$i]
        }
        '^--manifest$|^-Manifest$' {
            $i++
            if ($i -ge $args.Count) { throw 'missing --manifest path' }
            $Manifest = [string]$args[$i]
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
    Write-Error 'live host inspect not implemented (T19). use --dry-run --fixture <json>'
    exit 2
}

if ([string]::IsNullOrWhiteSpace($Fixture) -or -not (Test-Path -LiteralPath $Fixture -PathType Leaf)) {
    Write-Error 'missing --fixture path'
    exit 2
}

if (-not (Test-Path -LiteralPath $Manifest -PathType Leaf)) {
    Write-Error "manifest missing: $Manifest"
    exit 2
}

if ([string]::IsNullOrWhiteSpace($LabBin)) {
    $cmd = Get-Command mmd-lab -ErrorAction SilentlyContinue
    if ($cmd) {
        $LabBin = $cmd.Source
    } elseif (Test-Path -LiteralPath (Join-Path $env:USERPROFILE '.local\bin\mmd-lab.exe')) {
        $LabBin = Join-Path $env:USERPROFILE '.local\bin\mmd-lab.exe'
    } else {
        Write-Error 'mmd-lab not found; set --lab-bin or MMD_LAB_BIN'
        exit 2
    }
}

& $LabBin attest-windows --manifest $Manifest --observed $Fixture
exit $LASTEXITCODE
