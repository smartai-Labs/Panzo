param(
    [Parameter(Mandatory = $true)]
    [string]$SourceProject,
    [string]$OutputDirectory = '.tmp',
    [string]$TargetDir = 'target',
    [string]$ExecutablePath,
    [switch]$NoBuild
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
    if (-not $NoBuild) {
        cargo build --workspace --target-dir $resolvedTargetDir
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    }

    $outputRoot = if ([System.IO.Path]::IsPathRooted($OutputDirectory)) {
        [System.IO.Path]::GetFullPath($OutputDirectory)
    } else {
        [System.IO.Path]::GetFullPath((Join-Path $workspaceRoot $OutputDirectory))
    }
    New-Item -ItemType Directory -Force -Path $outputRoot | Out-Null
    $runId = [guid]::NewGuid().ToString('N')
    $runRoot = Join-Path $outputRoot "editor-atomic-$runId"
    New-Item -ItemType Directory -Path $runRoot | Out-Null
    $baselineProject = Join-Path $runRoot 'baseline.panzo'
    Copy-Item -LiteralPath $source -Destination $baselineProject -Recurse

    # Normalize only the acceptance copy back to its Auto Draft so every crash stage,
    # including first-save Auto Draft creation, is exercised deterministically.
    $autoCamera = Join-Path $baselineProject 'tracks\camera.auto.json'
    $camera = Join-Path $baselineProject 'tracks\camera.json'
    if (Test-Path -LiteralPath $autoCamera) {
        Copy-Item -LiteralPath $autoCamera -Destination $camera -Force
        Remove-Item -LiteralPath $autoCamera -Force
    }
    $editDirectory = Join-Path $baselineProject 'edit'
    if (Test-Path -LiteralPath $editDirectory) {
        $resolvedEdit = [System.IO.Path]::GetFullPath($editDirectory)
        $resolvedBaseline = [System.IO.Path]::GetFullPath($baselineProject)
        if (-not $resolvedEdit.StartsWith(
            $resolvedBaseline + [System.IO.Path]::DirectorySeparatorChar,
            [System.StringComparison]::OrdinalIgnoreCase
        )) {
            throw "Refusing to remove edit directory outside acceptance Project: $resolvedEdit"
        }
        Remove-Item -LiteralPath $resolvedEdit -Recurse -Force
    }

    $executable = Join-Path $resolvedTargetDir 'debug\panzo-cli.exe'
    if (-not [string]::IsNullOrWhiteSpace($ExecutablePath)) {
        $executable = (Resolve-Path -LiteralPath $ExecutablePath).Path
    }
    $baselineJson = (& $executable editor-atomic-inspect $baselineProject | Out-String)
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    $baseline = $baselineJson | ConvertFrom-Json

    $controlProject = Join-Path $runRoot 'control.panzo'
    Copy-Item -LiteralPath $baselineProject -Destination $controlProject -Recurse
    $controlJson = (& $executable editor-atomic-save-probe $controlProject | Out-String)
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    $control = $controlJson | ConvertFrom-Json
    if ($control.stateFingerprint -eq $baseline.stateFingerprint -or $control.revision -ne 1) {
        throw 'ED-ATOMIC-001 setup failed: control Save did not create revision 1'
    }

    $stages = @(
        'auto-draft-staged',
        'camera-staged',
        'workbench-staged',
        'history-staged',
        'commit-marker-published',
        'auto-draft-materialized',
        'camera-materialized',
        'workbench-materialized',
        'history-materialized',
        'commit-marker-removed'
    )
    $results = @()
    foreach ($stage in $stages) {
        $caseProject = Join-Path $runRoot "$stage.panzo"
        Copy-Item -LiteralPath $baselineProject -Destination $caseProject -Recurse
        & $executable editor-atomic-crash-probe $caseProject $stage
        $crashExitCode = $LASTEXITCODE
        if ($crashExitCode -ne 86) {
            throw "ED-ATOMIC-001 ${stage}: crash probe exited $crashExitCode instead of 86"
        }

        $recoveredJson = (& $executable editor-atomic-inspect $caseProject | Out-String)
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
        $recovered = $recoveredJson | ConvertFrom-Json
        $committed = $stages.IndexOf($stage) -ge $stages.IndexOf('commit-marker-published')
        $expectedFingerprint = if ($committed) {
            $control.stateFingerprint
        } else {
            $baseline.stateFingerprint
        }
        if ($recovered.stateFingerprint -ne $expectedFingerprint) {
            throw "ED-ATOMIC-001 ${stage}: recovered state is neither the expected old nor new revision"
        }
        if ($recovered.pendingTransaction -or $recovered.transactionArtifacts -ne 0) {
            throw "ED-ATOMIC-001 ${stage}: transaction artifacts remain after recovery"
        }
        if ($recovered.sourceMediaHash -ne $baseline.sourceMediaHash -or
            $recovered.sourceCursorHash -ne $baseline.sourceCursorHash -or
            $recovered.sourceClicksHash -ne $baseline.sourceClicksHash) {
            throw "ED-ATOMIC-001 ${stage}: an immutable source asset changed"
        }
        $results += [ordered]@{
            stage = $stage
            crashExitCode = $crashExitCode
            committed = $committed
            recoveredRevision = $recovered.revision
            recoveredFingerprint = $recovered.stateFingerprint
            pendingTransaction = $recovered.pendingTransaction
            transactionArtifacts = $recovered.transactionArtifacts
        }
        Write-Host "ED-ATOMIC-001 passed: $stage -> revision $($recovered.revision)"
    }

    $evidencePath = Join-Path $runRoot 'editor-atomic-acceptance.json'
    [ordered]@{
        schemaVersion = 1
        passed = $true
        sourceProject = $source
        baseline = $baseline
        committedControl = $control
        crashExitCode = 86
        results = $results
    } | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $evidencePath -Encoding UTF8

    Write-Output "Editor atomic transaction acceptance passed: $($stages.Count)/$($stages.Count)"
    Write-Output "Evidence: $evidencePath"
}
finally {
    Pop-Location
}
