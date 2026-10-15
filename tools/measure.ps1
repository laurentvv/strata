# Measures scan time and memory of the GUI: starts strata.exe on a folder, samples the working set
# until the process is idle (CPU time stops growing), then reports the peak and the settled values.
#   pwsh tools/measure.ps1 -Path C:\Windows [-Exe target\release\strata.exe] [-MaxSeconds 120]
param(
    [Parameter(Mandatory)][string]$Path,
    [string]$Exe = "target\release\strata.exe",
    [int]$MaxSeconds = 120
)
$ErrorActionPreference = "Stop"
$p = Start-Process -FilePath $Exe -ArgumentList "`"$Path`"" -PassThru
$sw = [Diagnostics.Stopwatch]::StartNew()
$peak = 0L; $lastCpu = -1.0; $idleSince = $null; $busyUntil = $null
while ($sw.Elapsed.TotalSeconds -lt $MaxSeconds -and -not $p.HasExited) {
    Start-Sleep -Milliseconds 200
    $p.Refresh()
    $ws = $p.WorkingSet64
    if ($ws -gt $peak) { $peak = $ws }
    $cpu = $p.TotalProcessorTime.TotalSeconds
    if ($cpu - $lastCpu -gt 0.05) { $idleSince = $null; $busyUntil = $sw.Elapsed.TotalSeconds }
    elseif ($null -eq $idleSince) { $idleSince = $sw.Elapsed.TotalSeconds }
    $lastCpu = $cpu
    # Idle for 3 s after the scan started doing work: the scan is finished.
    if ($null -ne $idleSince -and $null -ne $busyUntil -and ($sw.Elapsed.TotalSeconds - $idleSince) -gt 3) { break }
}
$p.Refresh()
$settled = $p.WorkingSet64
$private = $p.PrivateMemorySize64
$scanSeconds = [math]::Round($busyUntil, 1)
if (-not $p.HasExited) { $p.Kill() }
"{0}: scan ~{1} s, peak working set {2:N0} MB, settled {3:N0} MB, private {4:N0} MB" -f `
    $Path, $scanSeconds, ($peak / 1MB), ($settled / 1MB), ($private / 1MB)
