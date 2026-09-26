param(
    [ValidateRange(5, 3600)]
    [int]$DurationSeconds = 1800,
    [ValidateRange(15, 300)]
    [int]$WatchdogGraceSeconds = 60,
    [string]$OutputDirectory = "",
    [string]$ExecutablePath,
    [switch]$NoStimulus,
    [switch]$NoBuild
)

$ErrorActionPreference = 'Stop'
. "$PSScriptRoot/validation-common.ps1"
$workspace = Split-Path -Parent $PSScriptRoot
if ([string]::IsNullOrWhiteSpace($OutputDirectory)) {
    $OutputDirectory = Join-Path $workspace '.tmp'
}
$OutputDirectory = Resolve-PanzoPath $OutputDirectory
if ([string]::IsNullOrWhiteSpace($ExecutablePath)) {
    $executable = Join-Path $workspace 'target\debug\panzo-cli.exe'
}
elseif ([System.IO.Path]::IsPathRooted($ExecutablePath)) {
    $executable = $ExecutablePath
}
else {
    $executable = Resolve-PanzoPath $ExecutablePath
}

$runId = [Guid]::NewGuid().ToString('N')
$projectRoot = Join-Path $OutputDirectory "m4-stability-$runId.panzo"
$stdoutPath = Join-Path $OutputDirectory "m4-stability-$runId.stdout.log"
$stderrPath = Join-Path $OutputDirectory "m4-stability-$runId.stderr.log"
$stimulusStdoutPath = Join-Path $OutputDirectory "m4-stimulus-$runId.stdout.log"
$stimulusStderrPath = Join-Path $OutputDirectory "m4-stimulus-$runId.stderr.log"
$recordingProcess = $null
$stimulusProcess = $null

New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
Push-Location $workspace
try {
    if (-not $NoBuild) {
        cargo build -p panzo-app
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    }
    if (-not (Test-Path -LiteralPath $executable -PathType Leaf)) {
        throw "Panzo executable does not exist: $executable"
    }

    if (-not $NoStimulus) {
        $stimulusArguments = @('stability-stimulus', ($DurationSeconds + 10))
        $stimulusProcess = Start-Process `
            -FilePath $executable `
            -WindowStyle Hidden `
            -ArgumentList $stimulusArguments `
            -PassThru `
            -RedirectStandardOutput $stimulusStdoutPath `
            -RedirectStandardError $stimulusStderrPath
        $stimulusDeadline = [DateTime]::UtcNow.AddSeconds(5)
        while ($true) {
            Start-Sleep -Milliseconds 100
            $stimulusProcess.Refresh()
            if ($stimulusProcess.HasExited) {
                $stimulusError = Get-Content -LiteralPath $stimulusStderrPath -Raw
                throw "M4 stability stimulus exited during startup: $stimulusError"
            }
            if ($stimulusProcess.MainWindowHandle -ne 0) { break }
            if ([DateTime]::UtcNow -ge $stimulusDeadline) {
                throw 'M4 stability stimulus did not create its window within 5 seconds'
            }
        }
        Write-Host 'M4 full-screen 60 Hz stability stimulus is active.'
    }

    Write-Host "M4 stability recording target: $projectRoot"
    Write-Host "Required duration: $DurationSeconds seconds"
    $arguments = @('record-probe', ('"{0}"' -f $projectRoot), $DurationSeconds)
    $recordingProcess = Start-Process `
        -FilePath $executable `
        -WindowStyle Hidden `
        -ArgumentList $arguments `
        -PassThru `
        -RedirectStandardOutput $stdoutPath `
        -RedirectStandardError $stderrPath
    $deadline = [DateTime]::UtcNow.AddSeconds($DurationSeconds + $WatchdogGraceSeconds)
    while (-not $recordingProcess.WaitForExit(500)) {
        if ([DateTime]::UtcNow -ge $deadline) {
            Stop-Process -Id $recordingProcess.Id -Force
            Wait-Process -Id $recordingProcess.Id -ErrorAction SilentlyContinue
            $sessionLogPath = Join-Path $projectRoot 'diagnostics\session.log'
            if (Test-Path -LiteralPath $sessionLogPath) {
                Write-Host 'Last recorder stages:'
                Get-Content -LiteralPath $sessionLogPath -Tail 20 |
                    ForEach-Object { Write-Host "  $_" }
            }
            throw "record-probe exceeded its $($DurationSeconds + $WatchdogGraceSeconds)-second watchdog"
        }
        $recordingProcess.Refresh()
    }
    $recordingProcess.WaitForExit()
    $recordingExitCode = $recordingProcess.ExitCode
    $recordingProcess = $null
    if (Test-Path -LiteralPath $stderrPath) {
        Get-Content -LiteralPath $stderrPath -Tail 20 | ForEach-Object { Write-Host $_ }
    }
    # Windows PowerShell can expose a null ExitCode for a redirected Start-Process even after
    # WaitForExit. The project probe below remains authoritative in that case.
    if ($null -ne $recordingExitCode -and $recordingExitCode -ne 0) {
        throw "record-probe failed with exit code $recordingExitCode"
    }

    if ($null -ne $stimulusProcess) {
        $stimulusProcess.Refresh()
        if (-not $stimulusProcess.HasExited) {
            [void]$stimulusProcess.CloseMainWindow()
            if (-not $stimulusProcess.WaitForExit(12000)) {
                Stop-Process -Id $stimulusProcess.Id -Force
                Wait-Process -Id $stimulusProcess.Id -ErrorAction SilentlyContinue
            }
        }
        $stimulusProcess = $null
        $stimulusOutput = Get-Content -LiteralPath $stimulusStdoutPath -Raw
        $stimulusError = Get-Content -LiteralPath $stimulusStderrPath -Raw
        if ($stimulusError) { Write-Host $stimulusError.TrimEnd() }
        Write-Host $stimulusOutput.TrimEnd()
        $stimulusTimerMatch = [regex]::Match($stimulusOutput, 'Timer ticks:\s+([1-9][0-9]*)')
        $stimulusPaintMatch = [regex]::Match($stimulusOutput, 'Paint calls:\s+([1-9][0-9]*)')
        $minimumStimulusFrames = [int64]$DurationSeconds * 50
        if ($stimulusOutput -notmatch 'Surface:\s+[1-9][0-9]*x[1-9][0-9]*' -or
            -not $stimulusTimerMatch.Success -or
            -not $stimulusPaintMatch.Success -or
            $stimulusOutput -notmatch 'Graceful close:\s+true' -or
            [int64]$stimulusTimerMatch.Groups[1].Value -lt $minimumStimulusFrames -or
            [int64]$stimulusPaintMatch.Groups[1].Value -lt $minimumStimulusFrames) {
            throw 'M4-STABILITY-002 failed: the full-screen animation stimulus evidence is incomplete'
        }
    }

    $previousErrorPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        $acceptanceJson = (& $executable stability-probe $projectRoot $DurationSeconds 2> (Join-Path $projectRoot 'diagnostics/stability-probe.stderr.txt') | Out-String)
        $stabilityExitCode = $LASTEXITCODE
    } finally { $ErrorActionPreference = $previousErrorPreference }
    $evidencePath = Join-Path $projectRoot 'diagnostics\stability-acceptance.json'
    $acceptanceJson | Set-Content -LiteralPath $evidencePath -Encoding UTF8
    Write-Host $acceptanceJson.TrimEnd()
    $acceptance = $acceptanceJson | ConvertFrom-Json
    if ($stabilityExitCode -ne 0) {
        throw "M4-STABILITY-001 failed: $($acceptance.failures -join '; ')"
    }

    $manifest = Get-Content -LiteralPath (Join-Path $projectRoot 'project.json') -Raw |
        ConvertFrom-Json
    $mediaPath = Join-Path $projectRoot $manifest.media.screen
    $decodeOutput = (& $executable decode-probe $mediaPath 3 2>&1 | Out-String)
    Write-Host $decodeOutput.TrimEnd()
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    # The fragmented MP4 sink does not emit a random-access index on every driver. Keep this
    # deterministic probe near the start; full-container timing is validated by stability-probe.
    $seekMilliseconds = [Math]::Min(1000, [Math]::Max(0, ($DurationSeconds - 1) * 1000))
    $seekOutput = (& $executable decode-seek-probe $mediaPath $seekMilliseconds 2>&1 | Out-String)
    Write-Host $seekOutput.TrimEnd()
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    if ($decodeOutput -notmatch 'Strictly increasing PTS:\s+true' -or
        $seekOutput -notmatch 'Indexed frames:\s+[1-9][0-9]*' -or
        $seekOutput -notmatch 'Selected BGRA frame bytes:\s+[1-9][0-9]*') {
        throw 'M4-STABILITY-001 failed: source media decode or indexed seek evidence is missing'
    }
    if (-not [bool]$acceptance.passed) {
        throw "M4-STABILITY-001 failed: $($acceptance.failures -join '; ')"
    }

    Write-Host "M4 stability acceptance passed: $projectRoot"
    Write-Host "Evidence: $evidencePath"
}
finally {
    if ($null -ne $recordingProcess) {
        $recordingProcess.Refresh()
        if (-not $recordingProcess.HasExited) {
            Stop-Process -Id $recordingProcess.Id -Force
            Wait-Process -Id $recordingProcess.Id -ErrorAction SilentlyContinue
        }
    }
    if ($null -ne $stimulusProcess) {
        $stimulusProcess.Refresh()
        if (-not $stimulusProcess.HasExited) {
            [void]$stimulusProcess.CloseMainWindow()
            if (-not $stimulusProcess.WaitForExit(3000)) {
                Stop-Process -Id $stimulusProcess.Id -Force
                Wait-Process -Id $stimulusProcess.Id -ErrorAction SilentlyContinue
            }
        }
    }
    Pop-Location
}
