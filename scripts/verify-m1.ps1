param(
    [ValidateRange(1, 3600)]
    [int]$DurationSeconds = 60,
    [ValidateRange(5, 120)]
    [int]$WatchdogGraceSeconds = 30,
    [string]$OutputDirectory = "",
    [switch]$SkipRecovery
)

$ErrorActionPreference = 'Stop'
. "$PSScriptRoot/validation-common.ps1"
$workspace = Split-Path -Parent $PSScriptRoot
if ([string]::IsNullOrWhiteSpace($OutputDirectory)) {
    $OutputDirectory = Join-Path $workspace '.tmp'
}
$OutputDirectory = Resolve-PanzoPath $OutputDirectory

New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
$projectName = 'm1-acceptance-{0}.panzo' -f ([guid]::NewGuid().ToString('N'))
$projectRoot = Join-Path $OutputDirectory $projectName
$executable = Join-Path $workspace 'target\debug\panzo-cli.exe'
$recordingProcess = $null

Push-Location $workspace
try {
    & (Join-Path $PSScriptRoot 'verify.ps1') -IncludeMediaProbe -IncludeInputProbe
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    Write-Host "M1 interactive recording target: $projectRoot"
    Write-Host "Move the pointer and click Left/Right/Middle during the recording."
    $arguments = @('record-probe', ('"{0}"' -f $projectRoot), $DurationSeconds)
    $recordingProcess = Start-Process -FilePath $executable -ArgumentList $arguments -PassThru -NoNewWindow
    $recordingDeadline = [DateTime]::UtcNow.AddSeconds($DurationSeconds + $WatchdogGraceSeconds)
    while (-not $recordingProcess.WaitForExit(250)) {
        if ([DateTime]::UtcNow -ge $recordingDeadline) {
            Stop-Process -Id $recordingProcess.Id -Force
            Wait-Process -Id $recordingProcess.Id -ErrorAction SilentlyContinue
            $sessionLog = Join-Path $projectRoot 'diagnostics\session.log'
            if (Test-Path -LiteralPath $sessionLog) {
                Write-Host 'Last recorder stages:'
                Get-Content -LiteralPath $sessionLog -Tail 20 | ForEach-Object { Write-Host "  $_" }
            }
            throw "record-probe exceeded its $($DurationSeconds + $WatchdogGraceSeconds)-second watchdog deadline"
        }
        $recordingProcess.Refresh()
    }
    $recordingProcess.WaitForExit()
    $recordingExitCode = $recordingProcess.ExitCode
    $recordingProcess = $null
    if ($recordingExitCode -ne 0) { exit $recordingExitCode }

    cargo run -p panzo-app --bin panzo-cli -- fmp4-inspect (Join-Path $projectRoot 'media\screen.mp4')
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    $manifest = Get-Content -LiteralPath (Join-Path $projectRoot 'project.json') -Raw | ConvertFrom-Json
    if ($manifest.state -ne 'ready') {
        throw "IT-REC-001 failed: expected ready project, got $($manifest.state)"
    }
    $sourceMediaPath = Join-Path $projectRoot 'media\screen.mp4'
    $decodeOutput = (& $executable decode-probe $sourceMediaPath 2 2>&1 | Out-String)
    Write-Host $decodeOutput.TrimEnd()
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    $expectedFrameSize = '{0}x{1}' -f `
        [int]$manifest.capture.contentSizePx.width, `
        [int]$manifest.capture.contentSizePx.height
    if ($decodeOutput -notmatch ('Frame size:\s+{0}\b' -f [regex]::Escape($expectedFrameSize))) {
        throw "IT-REC-006 failed: source video does not preserve native capture size $expectedFrameSize"
    }

    $metrics = Get-Content -LiteralPath (Join-Path $projectRoot 'diagnostics\metrics.json') -Raw | ConvertFrom-Json
    $mediaDurationSeconds = [double]$manifest.media.durationTick / 10000000.0
    if ($mediaDurationSeconds -le 0 -or [uint64]$metrics.capture.framesEncoded -eq 0) {
        throw 'IT-REC-001 failed: recorded media has no positive duration or encoded frames'
    }
    $averageEncodedFps = [double]$metrics.capture.framesEncoded / $mediaDurationSeconds
    if ($averageEncodedFps -gt 60.5) {
        throw ('IT-REC-001 failed: average encoded frame rate {0:N3} exceeds the 60 FPS target' -f $averageEncodedFps)
    }
    Write-Host ('M1 media frame rate: {0:N3} FPS ({1} encoded, {2} rate-limited)' -f `
        $averageEncodedFps, $metrics.capture.framesEncoded, $metrics.capture.droppedByFrameRateLimit)

    $cursorPath = Join-Path $projectRoot 'events\cursor.jsonl'
    $clickPath = Join-Path $projectRoot 'events\clicks.jsonl'
    $cursorEvents = @(Get-Content -LiteralPath $cursorPath | ForEach-Object { $_ | ConvertFrom-Json })
    $clickEvents = @(Get-Content -LiteralPath $clickPath | ForEach-Object { $_ | ConvertFrom-Json })
    if ($cursorEvents.Count -eq 0 -or $clickEvents.Count -eq 0) {
        throw 'IT-REC-002 failed: recording must contain cursor and click events'
    }
    foreach ($click in $clickEvents) {
        $anchor = $cursorEvents | Where-Object {
            $_.timeTick -eq $click.timeTick -and $_.kind -eq 'button-anchor'
        } | Select-Object -First 1
        if ($null -eq $anchor) {
            throw "IT-REC-002 failed: click $($click.id) has no exact cursor anchor"
        }
    }

    Write-Host "M1 project ready: $projectRoot"

    if (-not $SkipRecovery) {
        & (Join-Path $PSScriptRoot 'verify-m1-recovery.ps1') -CaptureSeconds 20 -OutputDirectory $OutputDirectory
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    }
}
finally {
    if ($null -ne $recordingProcess) {
        $recordingProcess.Refresh()
        if (-not $recordingProcess.HasExited) {
            Stop-Process -Id $recordingProcess.Id -Force
        }
    }
    Pop-Location
}
