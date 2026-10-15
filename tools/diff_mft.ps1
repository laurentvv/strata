# Lists the entries (down to depth 8) that the regular scan finds but the NTFS fast scan does not,
# and the other way round. Run it in an ELEVATED PowerShell after tools/validate_mft.ps1 shows a gap.
#   pwsh tools/diff_mft.ps1 -Drive D: [-Top 25] [-Exe target\release\strata.exe]
param(
    [Parameter(Mandatory)][string]$Drive,
    [int]$Top = 25,
    [string]$Exe = "target\release\strata.exe"
)
$ErrorActionPreference = "Stop"
$env:NO_COLOR = "1"
$id = [Security.Principal.WindowsIdentity]::GetCurrent()
if (-not (New-Object Security.Principal.WindowsPrincipal $id).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw "Run this script from an elevated (administrator) PowerShell."
}
$root = $Drive.TrimEnd('\') + '\'
$dir = Join-Path $env:TEMP "strata_mft_diff"
New-Item -ItemType Directory -Force $dir | Out-Null
$walkCsv = Join-Path $dir "walker.csv"
$fastCsv = Join-Path $dir "mft.csv"
Remove-Item $walkCsv, $fastCsv -ErrorAction SilentlyContinue
& $Exe --cli $root -d 0 --export $walkCsv | Out-Null
& $Exe --cli $root -d 0 --fast --export $fastCsv | Out-Null

function Load([string]$p) {
    $h = @{}
    foreach ($r in Import-Csv $p) { $h[$r.path] = $r }
    $h
}
$walk = Load $walkCsv
$fast = Load $fastCsv
"entries: walker {0:N0}, mft {1:N0}" -f $walk.Count, $fast.Count

$onlyWalk = $walk.Keys | Where-Object { -not $fast.ContainsKey($_) }
$onlyFast = $fast.Keys | Where-Object { -not $walk.ContainsKey($_) }
"only in the regular scan: {0:N0}; only in the fast scan: {1:N0}" -f @($onlyWalk).Count, @($onlyFast).Count
"--- largest entries only in the regular scan"
$onlyWalk | ForEach-Object { $walk[$_] } | Sort-Object { [int64]$_.size_bytes } -Descending | Select-Object -First $Top type, size_bytes, path | Format-Table -AutoSize
"--- entries only in the fast scan"
$onlyFast | ForEach-Object { $fast[$_] } | Select-Object -First $Top type, size_bytes, path | Format-Table -AutoSize

# Folders present in both whose file counts differ: where the gap is concentrated.
$diff = foreach ($k in $walk.Keys) {
    if ($fast.ContainsKey($k) -and $walk[$k].type -eq 'folder') {
        $d = [int64]$walk[$k].files - [int64]$fast[$k].files
        if ($d -ne 0) { [pscustomobject]@{ FilesWalker = $walk[$k].files; FilesMft = $fast[$k].files; Diff = $d; Path = $k } }
    }
}
"--- folders with the most differing file counts (includes ancestors)"
$diff | Sort-Object { [math]::Abs($_.Diff) } -Descending | Select-Object -First $Top | Format-Table -AutoSize
