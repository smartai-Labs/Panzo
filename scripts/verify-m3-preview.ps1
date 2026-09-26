param(
    [Parameter(Mandatory = $true)]
    [string]$ProjectRoot,
    [ValidateRange(500, 10000)]
    [int]$SmokeMilliseconds = 1500,
    [switch]$SkipFoundationVerification
)

$ErrorActionPreference = 'Stop'
$workspace = Split-Path -Parent $PSScriptRoot
if (-not [System.IO.Path]::IsPathRooted($ProjectRoot)) {
    $ProjectRoot = Join-Path $workspace $ProjectRoot
}
if (-not (Test-Path -LiteralPath $ProjectRoot -PathType Container)) {
    throw "M3 Preview project does not exist: $ProjectRoot"
}

$manifestPath = Join-Path $ProjectRoot 'project.json'
$manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
$mediaPath = Join-Path $ProjectRoot $manifest.media.screen
$cameraPath = Join-Path $ProjectRoot $manifest.tracks.camera
if (-not (Test-Path -LiteralPath $mediaPath -PathType Leaf)) {
    throw "M3 Preview source media does not exist: $mediaPath"
}
if (-not (Test-Path -LiteralPath $cameraPath -PathType Leaf)) {
    throw "M3 Preview camera track does not exist: $cameraPath"
}

$executable = Join-Path $workspace 'target\debug\panzo-cli.exe'
Push-Location $workspace
try {
    if (-not $SkipFoundationVerification) {
        & (Join-Path $PSScriptRoot 'verify.ps1')
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    }
    elseif (-not (Test-Path -LiteralPath $executable -PathType Leaf)) {
        cargo build -p panzo-app
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    }

    $seekOutput = (& $executable decode-seek-probe $mediaPath 1000 2>&1 | Out-String)
    Write-Host $seekOutput.TrimEnd()
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    if ($seekOutput -notmatch 'Indexed frames:\s+[1-9][0-9]*' -or
        $seekOutput -notmatch 'Selected BGRA frame bytes:\s+[1-9][0-9]*') {
        throw 'M3-PREVIEW-SEEK failed: source index or exact decoded target is missing'
    }

    $sessionOutput = (& $executable preview-session-probe $ProjectRoot 2>&1 | Out-String)
    Write-Host $sessionOutput.TrimEnd()
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    if ($sessionOutput -notmatch 'Deterministic start frame:\s+true' -or
        $sessionOutput -notmatch 'Camera diagnostics visible:\s+true') {
        throw 'M3-PREVIEW-SESSION failed: deterministic seek round-trip or diagnostics toggle failed'
    }

    $regenerateOutput1 = (& $executable camera-regenerate-probe $ProjectRoot 2>&1 | Out-String)
    Write-Host $regenerateOutput1.TrimEnd()
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    $cameraHash1 = (Get-FileHash -LiteralPath $cameraPath -Algorithm SHA256).Hash

    $regenerateOutput2 = (& $executable camera-regenerate-probe $ProjectRoot 2>&1 | Out-String)
    Write-Host $regenerateOutput2.TrimEnd()
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    $cameraHash2 = (Get-FileHash -LiteralPath $cameraPath -Algorithm SHA256).Hash
    if ($cameraHash1 -ne $cameraHash2) {
        throw 'M3-PREVIEW-CAMERA failed: repeated regeneration produced different camera hashes'
    }
    if (Test-Path -LiteralPath "$cameraPath.tmp") {
        throw 'M3-PREVIEW-CAMERA failed: atomic camera update left a temporary file'
    }

    $smokeOutput = (& $executable preview-smoke $ProjectRoot $SmokeMilliseconds 2>&1 | Out-String)
    Write-Host $smokeOutput.TrimEnd()
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    $timerMatch = [regex]::Match($smokeOutput, 'Timer ticks:\s+([0-9]+)')
    $renderMatch = [regex]::Match($smokeOutput, 'Rendered frames:\s+([0-9]+)')
    $tickMatch = [regex]::Match($smokeOutput, 'Last project tick:\s+([0-9]+)')
    $outputMatch = [regex]::Match($smokeOutput, 'Output:\s+([1-9][0-9]*)x([1-9][0-9]*)\s+\(source native\)')
    if ($smokeOutput -notmatch 'Window created:\s+true' -or
        $smokeOutput -notmatch 'Graceful close:\s+true' -or
        -not $outputMatch.Success -or
        -not $timerMatch.Success -or [int]$timerMatch.Groups[1].Value -le 0 -or
        -not $renderMatch.Success -or [int]$renderMatch.Groups[1].Value -lt 2 -or
        -not $tickMatch.Success -or [int64]$tickMatch.Groups[1].Value -le 0) {
        throw 'M3-PREVIEW-WINDOW failed: window lifecycle, timer, playback, or rendering did not advance'
    }

    $pausedSmokeOutput = (& $executable preview-paused-smoke $ProjectRoot $SmokeMilliseconds 2>&1 | Out-String)
    Write-Host $pausedSmokeOutput.TrimEnd()
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    $pausedTimerMatch = [regex]::Match($pausedSmokeOutput, 'Timer ticks:\s+([0-9]+)')
    $pausedRenderMatch = [regex]::Match($pausedSmokeOutput, 'Rendered frames:\s+([0-9]+)')
    $pausedPaintMatch = [regex]::Match($pausedSmokeOutput, 'Paint calls:\s+([0-9]+)')
    $pausedOutputMatch = [regex]::Match($pausedSmokeOutput, 'Output:\s+([1-9][0-9]*)x([1-9][0-9]*)\s+\(source native\)')
    if ($pausedSmokeOutput -notmatch 'Window created:\s+true' -or
        $pausedSmokeOutput -notmatch 'Graceful close:\s+true' -or
        -not $pausedOutputMatch.Success -or
        $pausedOutputMatch.Groups[1].Value -ne $outputMatch.Groups[1].Value -or
        $pausedOutputMatch.Groups[2].Value -ne $outputMatch.Groups[2].Value -or
        -not $pausedTimerMatch.Success -or [int]$pausedTimerMatch.Groups[1].Value -le 0 -or
        -not $pausedRenderMatch.Success -or [int]$pausedRenderMatch.Groups[1].Value -ne 1 -or
        -not $pausedPaintMatch.Success -or [int]$pausedPaintMatch.Groups[1].Value -gt 2 -or
        $pausedSmokeOutput -notmatch 'Last project tick:\s+0\b') {
        throw 'M3-PREVIEW-PAUSED failed: paused timer caused repeated rendering or painting'
    }

    Write-Host "M3 Preview acceptance passed: $ProjectRoot"
    Write-Host "Deterministic camera SHA256: $cameraHash2"
}
finally {
    Pop-Location
}
