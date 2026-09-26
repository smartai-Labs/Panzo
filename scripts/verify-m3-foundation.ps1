param(
    [Parameter(Mandatory = $true)]
    [string]$ProjectRoot,
    [ValidateRange(1, 30)]
    [int]$DurationSeconds = 2,
    [string]$OutputPath = ""
)

$ErrorActionPreference = 'Stop'
$workspace = Split-Path -Parent $PSScriptRoot
if (-not [System.IO.Path]::IsPathRooted($ProjectRoot)) {
    $ProjectRoot = Join-Path $workspace $ProjectRoot
}
if (-not (Test-Path -LiteralPath $ProjectRoot -PathType Container)) {
    throw "M3 project does not exist: $ProjectRoot"
}
if ([string]::IsNullOrWhiteSpace($OutputPath)) {
    $OutputPath = Join-Path $workspace ('.tmp\m3-export-{0}.mp4' -f ([guid]::NewGuid().ToString('N')))
}
elseif (-not [System.IO.Path]::IsPathRooted($OutputPath)) {
    $OutputPath = Join-Path $workspace $OutputPath
}

$outputDirectory = Split-Path -Parent $OutputPath
New-Item -ItemType Directory -Force -Path $outputDirectory | Out-Null
$manifestPath = Join-Path $ProjectRoot 'project.json'
$manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
$requestedDurationTick = [int64]$DurationSeconds * 10000000
$effectiveDurationTick = [Math]::Min([int64]$manifest.media.durationTick, $requestedDurationTick)
if ($effectiveDurationTick -le 0) {
    throw 'M3 project has no positive media duration'
}
$expectedFrames = [int][Math]::Ceiling(([double]$effectiveDurationTick * 60.0) / 10000000.0)
$hashTickMilliseconds = [int][Math]::Max(
    0,
    [Math]::Min(10000, [Math]::Floor(([double]$effectiveDurationTick - 1.0) / 10000.0))
)
$executable = Join-Path $workspace 'target\debug\panzo-cli.exe'

Push-Location $workspace
try {
    & (Join-Path $PSScriptRoot 'verify.ps1')
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    $hashOutput = (& $executable render-hash-probe $ProjectRoot $hashTickMilliseconds 2>&1 | Out-String)
    Write-Host $hashOutput.TrimEnd()
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    if ($hashOutput -notmatch 'Hashes match:\s+true') {
        throw 'IT-RENDER-001 failed: Preview and Export BGRA hashes differ'
    }

    $exportOutput = (& $executable export-probe $ProjectRoot $OutputPath $DurationSeconds 2>&1 | Out-String)
    Write-Host $exportOutput.TrimEnd()
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    if ($exportOutput -notmatch ('Frames:\s+{0}\b' -f $expectedFrames)) {
        throw "IT-EXPORT-001 failed: expected $expectedFrames CFR frames"
    }
    if ($exportOutput -notmatch 'Trailing bytes:\s+0\b') {
        throw 'IT-EXPORT-001 failed: exported fMP4 has trailing bytes'
    }

    & $executable fmp4-inspect $OutputPath
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    $decodeOutput = (& $executable decode-probe $OutputPath ($expectedFrames + 1) 2>&1 | Out-String)
    Write-Host $decodeOutput.TrimEnd()
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    if ($decodeOutput -notmatch ('Decoded frames:\s+{0}\b' -f $expectedFrames)) {
        throw "IT-EXPORT-001 failed: expected to decode $expectedFrames frames"
    }
    if ($decodeOutput -notmatch 'Frame size:\s+1920x1080' -or
        $decodeOutput -notmatch 'Strictly increasing PTS:\s+true') {
        throw 'IT-EXPORT-001 failed: decoded output geometry or PTS is invalid'
    }

    $temporaryOutput = "$OutputPath.panzo-part.mp4"
    if (Test-Path -LiteralPath $temporaryOutput) {
        throw "M3 exporter left an incomplete temporary file: $temporaryOutput"
    }
    Write-Host "M3 Render Foundation acceptance passed: $OutputPath"
}
finally {
    Pop-Location
}
