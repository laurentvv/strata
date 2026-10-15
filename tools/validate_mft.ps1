# Compares the NTFS fast scan (--fast) with the regular scan on a whole drive.
# Must run in an ELEVATED PowerShell (the fast scan reads \\.\X: directly).
#   pwsh tools/validate_mft.ps1 -Drive D: [-Exe target\release\strata.exe]
# Totals can differ slightly: the regular scan cannot read protected folders (counted as errors) and
# the drive may change between the two runs. Large gaps point to a parsing problem.
param(
    [Parameter(Mandatory)][string]$Drive,
    [string]$Exe = "target\release\strata.exe"
)
$ErrorActionPreference = "Stop"
$env:NO_COLOR = "1"
$id = [Security.Principal.WindowsIdentity]::GetCurrent()
if (-not (New-Object Security.Principal.WindowsPrincipal $id).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw "Run this script from an elevated (administrator) PowerShell."
}
$root = $Drive.TrimEnd('\') + '\'

function Run-Scan([string[]]$extra) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $out = & $Exe --cli $root -d 0 @extra 2>&1 | Out-String
    $sw.Stop()
    $m = [regex]::Match($out, 'Scanned:\s+(?<size>.+?) \((?<disk>.+?) on disk\) in (?<files>\d+) files across (?<dirs>\d+) directories; (?<err>\d+) errors')
    [pscustomobject]@{
        Mode    = if ($out -match 'NTFS Master File Table') { 'MFT' } else { 'walker' }
        Seconds = [math]::Round($sw.Elapsed.TotalSeconds, 1)
        Size    = $m.Groups['size'].Value
        OnDisk  = $m.Groups['disk'].Value
        Files   = [int64]$m.Groups['files'].Value
        Dirs    = [int64]$m.Groups['dirs'].Value
        Errors  = $m.Groups['err'].Value
        Raw     = $out
    }
}

$walk = Run-Scan @()
$fast = Run-Scan @('--fast')
$walk, $fast | Format-Table Mode, Seconds, Size, OnDisk, Files, Dirs, Errors -AutoSize
if ($fast.Mode -ne 'MFT') {
    Write-Warning "The fast scan fell back to the walker:"
    $fast.Raw -split "`n" | Select-String -Pattern 'Fast NTFS' | ForEach-Object { $_.Line.Trim() }
    return
}
"Files: fast {0:N0} vs walker {1:N0} ({2:P2} difference)" -f $fast.Files, $walk.Files, (($fast.Files - $walk.Files) / [math]::Max(1, $walk.Files))
"Dirs:  fast {0:N0} vs walker {1:N0} ({2:P2} difference)" -f $fast.Dirs, $walk.Dirs, (($fast.Dirs - $walk.Dirs) / [math]::Max(1, $walk.Dirs))
