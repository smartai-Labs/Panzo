param(
    [Parameter(Mandatory = $true)]
    [string]$SourceProject,
    [int]$SmokeMilliseconds = 750,
    [string]$TargetDir = 'target',
    [switch]$SkipAtomic
)

$ErrorActionPreference = 'Stop'
$workspaceRoot = Split-Path -Parent $PSScriptRoot
$source = (Resolve-Path -LiteralPath $SourceProject).Path
if (-not $source.EndsWith('.panzo', [System.StringComparison]::OrdinalIgnoreCase) -or
    -not (Test-Path -LiteralPath (Join-Path $source 'project.json'))) {
    throw "SourceProject must be a Panzo Project directory: $source"
}

Push-Location $workspaceRoot
try {
    $resolvedTargetDir = if ([System.IO.Path]::IsPathRooted($TargetDir)) {
        [System.IO.Path]::GetFullPath($TargetDir)
    } else {
        [System.IO.Path]::GetFullPath((Join-Path $workspaceRoot $TargetDir))
    }
    cargo build --workspace --target-dir $resolvedTargetDir
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    $runId = [guid]::NewGuid().ToString('N')
    $projectRoot = Join-Path $workspaceRoot ".tmp\editor-acceptance-$runId.panzo"
    $exportPath = Join-Path $workspaceRoot ".tmp\editor-acceptance-$runId-1080p60.mp4"
    Copy-Item -LiteralPath $source -Destination $projectRoot -Recurse

    $executable = Join-Path $resolvedTargetDir 'debug\panzo-cli.exe'
    $editJson = (& $executable editor-acceptance-probe $projectRoot | Out-String)
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    $edit = $editJson | ConvertFrom-Json
    if (-not $edit.checks.autoDraftPreserved -or
        -not $edit.checks.editPersisted -or
        -not $edit.checks.stylePersisted -or
        -not $edit.checks.assets.backgroundAssetPersisted) {
        throw 'ED-PROJECT-001 failed: editor state did not persist or Auto Draft changed'
    }

    $renderOutput = (& $executable render-hash-probe $projectRoot 1000 | Out-String)
    Write-Host $renderOutput.TrimEnd()
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    if ($renderOutput -notmatch 'Hashes match:\s+true') {
        throw 'ED-RENDER-001 failed: Preview and Export render hashes differ'
    }

    $exportOutput = (& $executable export-probe $projectRoot $exportPath 2 | Out-String)
    Write-Host $exportOutput.TrimEnd()
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    $decodeOutput = (& $executable decode-probe $exportPath 120 | Out-String)
    Write-Host $decodeOutput.TrimEnd()
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    if ($decodeOutput -notmatch 'Frame size:\s+1920x1080' -or
        $decodeOutput -notmatch 'Strictly increasing PTS:\s+true') {
        throw 'ED-EXPORT-001 failed: edited export cannot be decoded as 1080p with monotonic PTS'
    }

    $sessionOutput = (& $executable preview-session-probe $projectRoot 23759628 | Out-String)
    Write-Host $sessionOutput.TrimEnd()
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    if ($sessionOutput -notmatch 'Scrub poll count:\s+[01]' -or
        $sessionOutput -notmatch 'Seek project/source tick:\s+23759628/23759628' -or
        $sessionOutput -notmatch 'Deterministic start frame:\s+true') {
        throw 'ED-SCRUB-001 failed: fast scrub preview or exact release seek regressed'
    }

    $smokeOutput = (& $executable preview-smoke $projectRoot $SmokeMilliseconds | Out-String)
    Write-Host $smokeOutput.TrimEnd()
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    if ($smokeOutput -notmatch 'Window created:\s+true' -or
        $smokeOutput -notmatch 'Graceful close:\s+true' -or
        $smokeOutput -notmatch 'Background decodes:\s+1' -or
        $smokeOutput -notmatch 'Background texture uploads:\s+1' -or
        $smokeOutput -notmatch 'GPU direct preview:\s+true' -or
        $smokeOutput -notmatch 'GPU presentations:\s+[1-9][0-9]*' -or
        $smokeOutput -notmatch 'CPU readbacks:\s+0' -or
        $smokeOutput -notmatch 'Output texture allocations:\s+0') {
        throw 'ED-UI-001 failed: Editor Workbench smoke evidence is incomplete'
    }

    $pausedSmokeOutput = (& $executable preview-paused-smoke $projectRoot $SmokeMilliseconds | Out-String)
    Write-Host $pausedSmokeOutput.TrimEnd()
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    if ($pausedSmokeOutput -notmatch 'Rendered frames:\s+1' -or
        $pausedSmokeOutput -notmatch 'Background decodes:\s+1' -or
        $pausedSmokeOutput -notmatch 'GPU direct preview:\s+true' -or
        $pausedSmokeOutput -notmatch 'GPU presentations:\s+1' -or
        $pausedSmokeOutput -notmatch 'CPU readbacks:\s+0' -or
        $pausedSmokeOutput -notmatch 'Output texture allocations:\s+0' -or
        $pausedSmokeOutput -notmatch 'Graceful close:\s+true') {
        throw 'ED-PERF-001 failed: paused Editor redrew or recreated render resources'
    }

    $trimSmokeMilliseconds = [Math]::Max($SmokeMilliseconds, 5000)
    $trimSmokeOutput = (& $executable preview-trim-smoke $projectRoot $trimSmokeMilliseconds | Out-String)
    Write-Host $trimSmokeOutput.TrimEnd()
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    $trimElapsedMatch = [regex]::Match($trimSmokeOutput, 'Trim action elapsed:\s+([0-9]+)\s+us')
    if ($trimSmokeOutput -notmatch 'Trim smoke status:\s+passed-incrementally' -or
        -not $trimElapsedMatch.Success -or
        [uint64]$trimElapsedMatch.Groups[1].Value -ge 100000) {
        throw 'ED-TRIM-UI-001 failed: Camera Segment trim blocked the UI or bypassed incremental preview'
    }

    $queuedPlaybackMilliseconds = [Math]::Max($SmokeMilliseconds, 2000)
    $queuedPlaybackOutput = (& $executable preview-queued-playback-smoke $projectRoot $queuedPlaybackMilliseconds | Out-String)
    Write-Host $queuedPlaybackOutput.TrimEnd()
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    $queuedPlaybackLatencyMatch = [regex]::Match(
        $queuedPlaybackOutput,
        'Queued playback latency:\s+([0-9]+)\s+us'
    )
    if ($queuedPlaybackOutput -notmatch 'Queued playback completed:\s+true' -or
        -not $queuedPlaybackLatencyMatch.Success -or
        [uint64]$queuedPlaybackLatencyMatch.Groups[1].Value -ge 750000) {
        throw 'ED-PLAYBACK-001 failed: playback after an incremental seek was lost or delayed'
    }

    $atomicOutput = ''
    if (-not $SkipAtomic) {
        $atomicOutput = (& (Join-Path $PSScriptRoot 'verify-editor-atomic.ps1') `
            -SourceProject $source `
            -OutputDirectory (Join-Path $workspaceRoot '.tmp') `
            -TargetDir $resolvedTargetDir | Out-String)
        Write-Host $atomicOutput.TrimEnd()
        if ($LASTEXITCODE -ne 0 -or
            $atomicOutput -notmatch 'Editor atomic transaction acceptance passed:\s+10/10') {
            throw 'ED-ATOMIC-001 failed: crash-safe editor transaction matrix did not pass'
        }
    }

    $evidencePath = Join-Path $projectRoot 'diagnostics\editor-acceptance.json'
    [ordered]@{
        schemaVersion = 1
        passed = $true
        sourceProject = $source
        acceptanceProject = $projectRoot
        editedExport = $exportPath
        edit = $edit
        renderHashEvidence = $renderOutput.Trim()
        exportEvidence = $exportOutput.Trim()
        decodeEvidence = $decodeOutput.Trim()
        scrubSessionEvidence = $sessionOutput.Trim()
        uiEvidence = $smokeOutput.Trim()
        pausedUiEvidence = $pausedSmokeOutput.Trim()
        trimUiEvidence = $trimSmokeOutput.Trim()
        queuedPlaybackEvidence = $queuedPlaybackOutput.Trim()
        atomicTransactionEvidence = $atomicOutput.Trim()
    } | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $evidencePath -Encoding UTF8

    Write-Host "Editor Workbench MVP acceptance passed: $projectRoot"
    Write-Host "Edited export: $exportPath"
    Write-Host "Evidence: $evidencePath"
}
finally {
    Pop-Location
}
