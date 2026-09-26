param(
    [ValidateRange(5, 300)]
    [int]$CaptureSeconds = 20,
    [string]$OutputDirectory = ""
)

$ErrorActionPreference = 'Stop'
$workspace = Split-Path -Parent $PSScriptRoot
if ([string]::IsNullOrWhiteSpace($OutputDirectory)) {
    $OutputDirectory = Join-Path $workspace '.tmp'
}
New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
$projectName = 'm1-recovery-{0}.panzo' -f ([guid]::NewGuid().ToString('N'))
$projectRoot = Join-Path $OutputDirectory $projectName
$executable = Join-Path $workspace 'target\debug\panzo-cli.exe'
$child = $null

Push-Location $workspace
try {
    cargo build -p panzo-app
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    $arguments = @('record-probe', ('"{0}"' -f $projectRoot), '600')
    $child = Start-Process -FilePath $executable -ArgumentList $arguments -PassThru -WindowStyle Hidden
    $lockPath = Join-Path $projectRoot 'recording.lock'
    $sessionLogPath = Join-Path $projectRoot 'diagnostics\session.log'
    $startupDeadline = [DateTime]::UtcNow.AddSeconds(15)
    while ($true) {
        $epochEstablished = (Test-Path -LiteralPath $sessionLogPath) -and
            (Select-String -LiteralPath $sessionLogPath -SimpleMatch 'stage=epoch-established' -Quiet)
        if ((Test-Path -LiteralPath $lockPath) -and $epochEstablished) {
            break
        }
        if ($child.HasExited) {
            throw "record-probe exited before the WGC epoch was established (exit $($child.ExitCode))"
        }
        if ([DateTime]::UtcNow -ge $startupDeadline) {
            if (Test-Path -LiteralPath $sessionLogPath) {
                Get-Content -LiteralPath $sessionLogPath -Tail 20 | ForEach-Object { Write-Host "  $_" }
            }
            throw 'record-probe did not establish the WGC epoch within 15 seconds'
        }
        Start-Sleep -Milliseconds 100
        $child.Refresh()
    }

    Start-Sleep -Seconds $CaptureSeconds
    $child.Refresh()
    if (-not $child.HasExited) {
        Stop-Process -Id $child.Id -Force
        Wait-Process -Id $child.Id -ErrorAction SilentlyContinue
    }
    $child = $null

    cargo run -p panzo-app --bin panzo-cli -- recovery-scan $OutputDirectory
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    $manifestPath = Join-Path $projectRoot 'project.json'
    $manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
    if ($manifest.state -ne 'recovered') {
        throw "IT-RECOVERY-001 failed: expected recovered, got $($manifest.state)"
    }
    $minimumDurationTick = [int64]([Math]::Max(0, $CaptureSeconds - 2)) * 10000000
    if ([int64]$manifest.media.durationTick -lt $minimumDurationTick) {
        throw "IT-RECOVERY-001 failed: recovered duration $($manifest.media.durationTick) is below $minimumDurationTick"
    }
    if (Test-Path -LiteralPath $lockPath) {
        throw 'IT-RECOVERY-001 failed: recording.lock remains after recovery'
    }
    Get-Content -LiteralPath (Join-Path $projectRoot 'events\cursor.jsonl') |
        ForEach-Object { $_ | ConvertFrom-Json | Out-Null }
    Get-Content -LiteralPath (Join-Path $projectRoot 'events\clicks.jsonl') |
        ForEach-Object { $_ | ConvertFrom-Json | Out-Null }

    Write-Host "M1 recovery project recovered: $projectRoot"
}
finally {
    if ($null -ne $child) {
        $child.Refresh()
        if (-not $child.HasExited) {
            Stop-Process -Id $child.Id -Force
        }
    }
    Pop-Location
}
