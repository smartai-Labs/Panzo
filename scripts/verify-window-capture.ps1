param(
    [ValidateRange(2, 30)]
    [int]$DurationSeconds = 3,
    [ValidateRange(10, 60)]
    [int]$WatchdogSeconds = 30
)

$ErrorActionPreference = 'Stop'
$workspace = Split-Path -Parent $PSScriptRoot
$runId = [guid]::NewGuid().ToString('N')
$runRoot = Join-Path $workspace ".tmp\window-capture-acceptance-$runId"
$projectRoot = Join-Path $runRoot 'window-capture.panzo'
$cli = Join-Path $workspace 'target\debug\panzo-cli.exe'
$stimulusTitle = 'Panzo M4 60 Hz Stability Stimulus'

New-Item -ItemType Directory -Path $runRoot -Force | Out-Null

Push-Location $workspace
try {
    cargo build -p panzo-app --bin panzo-cli
    if ($LASTEXITCODE -ne 0) {
        throw "Debug build failed with exit code $LASTEXITCODE"
    }

    $stimulusDuration = [Math]::Max($DurationSeconds + 5, 8)
    $stimulus = Start-Process -FilePath $cli `
        -ArgumentList @('stability-stimulus', $stimulusDuration) `
        -PassThru `
        -WindowStyle Hidden

    try {
        $enumerated = $false
        $enumerationDeadline = [DateTime]::UtcNow.AddSeconds(5)
        while ([DateTime]::UtcNow -lt $enumerationDeadline) {
            Start-Sleep -Milliseconds 150
            $windowList = (& $cli window-list | Out-String)
            if ($LASTEXITCODE -eq 0 -and $windowList.Contains($stimulusTitle)) {
                $enumerated = $true
                break
            }
        }
        if (-not $enumerated) {
            throw 'WC-ENUM-001 failed: the visible stimulus window was not enumerated'
        }

        $quotedProjectRoot = '"' + $projectRoot.Replace('"', '\"') + '"'
        $quotedStimulusTitle = '"' + $stimulusTitle.Replace('"', '\"') + '"'
        $record = Start-Process -FilePath $cli `
            -ArgumentList @('window-record-probe', $quotedProjectRoot, $quotedStimulusTitle, $DurationSeconds) `
            -PassThru `
            -WindowStyle Hidden
        if (-not $record.WaitForExit($WatchdogSeconds * 1000)) {
            throw "Window Capture exceeded its $WatchdogSeconds-second watchdog"
        }
        $record.Refresh()
        if ($record.ExitCode -ne 0) {
            throw "Window Capture failed with exit code $($record.ExitCode)"
        }
    }
    finally {
        if (-not $stimulus.HasExited) {
            $null = $stimulus.WaitForExit(10000)
        }
    }

    $manifestPath = Join-Path $projectRoot 'project.json'
    if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) {
        throw 'WC-PROJECT-001 failed: project.json is missing'
    }
    $manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
    if ($manifest.state -ne 'ready') {
        throw "WC-PROJECT-001 failed: project state is $($manifest.state)"
    }
    if ($manifest.capture.kind -ne 'window') {
        throw "WC-META-001 failed: capture kind is $($manifest.capture.kind)"
    }
    if ([string]::IsNullOrWhiteSpace($manifest.capture.windowId) -or
        $manifest.capture.windowTitle -ne $stimulusTitle) {
        throw 'WC-META-001 failed: window identity metadata is incomplete'
    }
    if ($manifest.capture.cursorCapturedInVideo) {
        throw 'WC-CURSOR-001 failed: system cursor remained in the source video'
    }
    if (($manifest.capture.contentSizePx.width % 2) -ne 0 -or
        ($manifest.capture.contentSizePx.height % 2) -ne 0) {
        throw 'WC-SIZE-001 failed: encoded window dimensions are not NV12-safe'
    }

    $mediaPath = Join-Path $projectRoot $manifest.media.screen
    $decodeEvidence = (& $cli decode-probe $mediaPath 300 | Out-String)
    if ($LASTEXITCODE -ne 0) {
        throw 'WC-DECODE-001 failed: recorded window media could not be decoded'
    }
    $sizePattern = 'Frame size:\s*' + $manifest.capture.contentSizePx.width + 'x' + $manifest.capture.contentSizePx.height
    if ($decodeEvidence -notmatch $sizePattern -or $decodeEvidence -notmatch 'Strictly increasing PTS:\s*true') {
        throw 'WC-DECODE-001 failed: decoded size or timestamps do not match the manifest'
    }

    $metricsPath = Join-Path $projectRoot 'diagnostics\metrics.json'
    $metrics = Get-Content -LiteralPath $metricsPath -Raw | ConvertFrom-Json
    if ($metrics.capture.framesEncoded -lt 2) {
        throw 'WC-RECORD-001 failed: hardware recording evidence is incomplete'
    }

    $evidence = [ordered]@{
        schemaVersion = 1
        passed = $true
        projectRoot = $projectRoot
        captureKind = $manifest.capture.kind
        windowId = $manifest.capture.windowId
        windowTitle = $manifest.capture.windowTitle
        contentWidth = $manifest.capture.contentSizePx.width
        contentHeight = $manifest.capture.contentSizePx.height
        cursorCapturedInVideo = $manifest.capture.cursorCapturedInVideo
        framesReceived = $metrics.capture.framesReceived
        framesEncoded = $metrics.capture.framesEncoded
        durationTick = $manifest.media.durationTick
        encoder = 'hardware H.264 required by recorder invariant'
        decoderEvidence = $decodeEvidence.Trim()
    }
    $evidencePath = Join-Path $projectRoot 'diagnostics\window-capture-acceptance.json'
    $evidence | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $evidencePath -Encoding utf8
    Write-Output "Window Capture acceptance passed: $projectRoot"
    Write-Output "Evidence: $evidencePath"
}
finally {
    Pop-Location
}
