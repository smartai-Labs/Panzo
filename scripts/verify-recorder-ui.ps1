param(
    [ValidateRange(1000, 10000)]
    [int]$RecordingMilliseconds = 2000,
    [ValidateRange(10, 120)]
    [int]$WatchdogSeconds = 45,
    [string]$ExecutablePath,
    [switch]$NoBuild
)

$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'validation-common.ps1')
$workspace = Split-Path -Parent $PSScriptRoot
if ([string]::IsNullOrWhiteSpace($ExecutablePath)) {
    $executable = Join-Path $workspace 'target\debug\panzo-cli.exe'
}
elseif ([System.IO.Path]::IsPathRooted($ExecutablePath)) {
    $executable = $ExecutablePath
}
else {
    $executable = Join-Path $workspace $ExecutablePath
}
$runId = [Guid]::NewGuid().ToString('N')
$projectLibrary = Join-Path $workspace ".tmp\recorder-ui-$runId"
$stdoutPath = Join-Path $projectLibrary 'smoke.stdout.log'
$stderrPath = Join-Path $projectLibrary 'smoke.stderr.log'

New-Item -ItemType Directory -Path $projectLibrary -Force | Out-Null
Push-Location $workspace
try {
    if (-not $NoBuild) {
        cargo build -p panzo-app
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    }
    if (-not (Test-Path -LiteralPath $executable -PathType Leaf)) {
        throw "Panzo executable does not exist: $executable"
    }

    $arguments = @(
        'recorder-smoke',
        $projectLibrary,
        $RecordingMilliseconds
    )
    $smoke = Invoke-PanzoProcess `
        -Executable $executable `
        -Arguments $arguments `
        -WorkingDirectory $workspace `
        -StdoutPath $stdoutPath `
        -StderrPath $stderrPath `
        -TimeoutSeconds $WatchdogSeconds
    $stdout = [string]$smoke.output
    $stderr = [string]$smoke.errors
    if ($stderr) { Write-Host $stderr.TrimEnd() }
    Write-Host $stdout.TrimEnd()
    if ($smoke.watchdogExpired) {
        throw "Recorder Window smoke exceeded its $WatchdogSeconds-second watchdog; see $stderrPath"
    }
    if ($null -eq $smoke.exitCode -or $smoke.exitCode -ne 0) {
        throw "Recorder Window smoke exited with code $($smoke.exitCode); see $stderrPath"
    }
    $selectedMatch = [regex]::Match($stdout, 'Selected monitor:\s+([1-9][0-9]*)x([1-9][0-9]*)\s+@\s+60 FPS')
    $projectMatch = [regex]::Match($stdout, 'Project:\s+(.+\.panzo)')
    $exportMatch = [regex]::Match($stdout, 'Export output:\s+(.+\.mp4)')
    $durationMatch = [regex]::Match($stdout, 'Duration tick:\s+([1-9][0-9]*)')
    $framesMatch = [regex]::Match($stdout, 'Frames encoded:\s+([1-9][0-9]*)')
    $exportFramesMatch = [regex]::Match($stdout, 'Export frames:\s+([1-9][0-9]*)')
    $exportBytesMatch = [regex]::Match($stdout, 'Export bytes:\s+([1-9][0-9]*)')
    if ($stdout -notmatch 'Window created:\s+true' -or
        $stdout -notmatch 'Recording started:\s+true' -or
        $stdout -notmatch 'Stop requested:\s+true' -or
        $stdout -notmatch 'Project ready:\s+true' -or
        $stdout -notmatch 'Export completed:\s+true' -or
        $stdout -notmatch 'Error:\s+none' -or
        $stdout -notmatch 'Graceful close:\s+true' -or
        -not $selectedMatch.Success -or
        -not $projectMatch.Success -or
        -not $exportMatch.Success -or
        -not $durationMatch.Success -or
        -not $framesMatch.Success -or
        -not $exportFramesMatch.Success -or
        -not $exportBytesMatch.Success) {
        throw 'RECORDER-UI-001 failed: Record, Stop, Finalize, Project Ready, or Export evidence is missing'
    }

    $projectRoot = $projectMatch.Groups[1].Value.Trim()
    $manifestPath = Join-Path $projectRoot 'project.json'
    if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) {
        throw "RECORDER-UI-001 failed: project manifest is missing: $manifestPath"
    }
    $manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
    if ($manifest.state -ne 'ready') {
        throw "RECORDER-UI-001 failed: project state is $($manifest.state), expected ready"
    }
    if ([int]$manifest.capture.contentSizePx.width -ne [int]$selectedMatch.Groups[1].Value -or
        [int]$manifest.capture.contentSizePx.height -ne [int]$selectedMatch.Groups[2].Value) {
        throw 'RECORDER-UI-002 failed: manifest capture size differs from selected monitor native size'
    }

    $mediaPath = Join-Path $projectRoot $manifest.media.screen
    $decodeOutput = (& $executable decode-probe $mediaPath 3 2>&1 | Out-String)
    Write-Host $decodeOutput.TrimEnd()
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    $expectedSize = "Frame size:\s+$($selectedMatch.Groups[1].Value)x$($selectedMatch.Groups[2].Value)"
    if ($decodeOutput -notmatch $expectedSize -or
        $decodeOutput -notmatch 'Decoded frames:\s+[1-9][0-9]*' -or
        $decodeOutput -notmatch 'Strictly increasing PTS:\s+true') {
        throw 'RECORDER-UI-002 failed: recorded media is not native-size decodable video'
    }

    $exportPath = $exportMatch.Groups[1].Value.Trim()
    if (-not (Test-Path -LiteralPath $exportPath -PathType Leaf)) {
        throw "RECORDER-UI-003 failed: export output is missing: $exportPath"
    }
    $exportDecodeOutput = (& $executable decode-probe $exportPath 3 2>&1 | Out-String)
    Write-Host $exportDecodeOutput.TrimEnd()
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    if ($exportDecodeOutput -notmatch 'Frame size:\s+1920x1080' -or
        $exportDecodeOutput -notmatch 'Decoded frames:\s+[1-9][0-9]*' -or
        $exportDecodeOutput -notmatch 'Strictly increasing PTS:\s+true') {
        throw 'RECORDER-UI-003 failed: exported MP4 is not decodable 1080p video'
    }

    $reopenStdoutPath = Join-Path $projectLibrary 'open-last.stdout.log'
    $reopenStderrPath = Join-Path $projectLibrary 'open-last.stderr.log'
    $reopenArguments = @(
        'recorder-open-last-smoke',
        $projectLibrary,
        500
    )
    $reopen = Invoke-PanzoProcess `
        -Executable $executable `
        -Arguments $reopenArguments `
        -WorkingDirectory $workspace `
        -StdoutPath $reopenStdoutPath `
        -StderrPath $reopenStderrPath `
        -TimeoutSeconds 10
    $reopenOutput = [string]$reopen.output
    $reopenError = [string]$reopen.errors
    if ($reopenError) { Write-Host $reopenError.TrimEnd() }
    Write-Host $reopenOutput.TrimEnd()
    if ($reopen.watchdogExpired) {
        throw "Recorder Window Open Last Project smoke exceeded its 10-second watchdog; see $reopenStderrPath"
    }
    if ($null -eq $reopen.exitCode -or $reopen.exitCode -ne 0) {
        throw "Recorder Window Open Last Project smoke exited with code $($reopen.exitCode); see $reopenStderrPath"
    }
    $loadedProjectMatch = [regex]::Match($reopenOutput, 'Loaded project:\s+(.+\.panzo)')
    if ($reopenOutput -notmatch 'Window created:\s+true' -or
        $reopenOutput -notmatch 'Last project loaded:\s+true' -or
        $reopenOutput -notmatch 'Project ready:\s+true' -or
        $reopenOutput -notmatch 'Graceful close:\s+true' -or
        $reopenOutput -notmatch 'Error:\s+none' -or
        -not $loadedProjectMatch.Success -or
        [System.IO.Path]::GetFullPath($loadedProjectMatch.Groups[1].Value.Trim()) -ne [System.IO.Path]::GetFullPath($projectRoot)) {
        throw 'RECORDER-UI-004 failed: application restart did not load the last Ready project'
    }

    Write-Host "Recorder Window record/export/reopen acceptance passed: $projectRoot"
}
finally {
    # Invoke-PanzoProcess owns each child and its job, saves both logs in finally,
    # and stops only that owned process tree on timeout or exceptional exit.
    Pop-Location
}
