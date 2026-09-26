param(
    [ValidateRange(1, 5)]
    [int]$Iterations = 5,
    [ValidateRange(5, 300)]
    [int]$CaptureSeconds = 20,
    [string]$OutputDirectory = "",
    [string]$ExecutablePath,
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
$runRoot = Join-Path $OutputDirectory "m4-recovery-$runId"
$child = $null
$results = @()

New-Item -ItemType Directory -Force -Path $runRoot | Out-Null
Push-Location $workspace
try {
    if (-not $NoBuild) {
        cargo build -p panzo-app
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    }
    if (-not (Test-Path -LiteralPath $executable -PathType Leaf)) {
        throw "Panzo executable does not exist: $executable"
    }

    for ($iteration = 1; $iteration -le $Iterations; $iteration++) {
        $projectRoot = Join-Path $runRoot ("forced-termination-{0:D2}.panzo" -f $iteration)
        $stdoutPath = Join-Path $runRoot ("record-{0:D2}.stdout.log" -f $iteration)
        $stderrPath = Join-Path $runRoot ("record-{0:D2}.stderr.log" -f $iteration)
        $arguments = @('record-probe', ('"{0}"' -f $projectRoot), 3600)
        $child = Start-Process `
            -FilePath $executable `
            -WindowStyle Hidden `
            -ArgumentList $arguments `
            -PassThru `
            -RedirectStandardOutput $stdoutPath `
            -RedirectStandardError $stderrPath

        $lockPath = Join-Path $projectRoot 'recording.lock'
        $sessionLogPath = Join-Path $projectRoot 'diagnostics\session.log'
        $startupDeadline = [DateTime]::UtcNow.AddSeconds(15)
        while ($true) {
            $epochEstablished = (Test-Path -LiteralPath $sessionLogPath) -and
                (Select-String -LiteralPath $sessionLogPath -SimpleMatch 'stage=epoch-established' -Quiet)
            if ((Test-Path -LiteralPath $lockPath) -and $epochEstablished) { break }
            $child.Refresh()
            if ($child.HasExited) {
                throw "iteration $iteration exited before epoch establishment (exit $($child.ExitCode))"
            }
            if ([DateTime]::UtcNow -ge $startupDeadline) {
                throw "iteration $iteration did not establish the WGC epoch within 15 seconds"
            }
            Start-Sleep -Milliseconds 100
        }

        Start-Sleep -Seconds $CaptureSeconds
        $child.Refresh()
        if (-not $child.HasExited) {
            Stop-Process -Id $child.Id -Force
            Wait-Process -Id $child.Id -ErrorAction SilentlyContinue
        }
        $child = $null

        $recoveryOutput = (& $executable recovery-scan $runRoot 2>&1 | Out-String)
        Write-Host $recoveryOutput.TrimEnd()
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

        $manifestPath = Join-Path $projectRoot 'project.json'
        $manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
        if ($manifest.state -ne 'recovered') {
            throw "M4-RECOVERY-001 iteration ${iteration}: expected recovered, got $($manifest.state)"
        }
        $minimumDurationTick = [int64]([Math]::Max(0, $CaptureSeconds - 2)) * 10000000
        if ([int64]$manifest.media.durationTick -lt $minimumDurationTick) {
            throw "M4-RECOVERY-001 iteration ${iteration}: duration $($manifest.media.durationTick) is below $minimumDurationTick"
        }
        if (Test-Path -LiteralPath $lockPath) {
            throw "M4-RECOVERY-001 iteration ${iteration}: recording.lock remains"
        }
        Get-Content -LiteralPath (Join-Path $projectRoot 'events\cursor.jsonl') |
            ForEach-Object { $_ | ConvertFrom-Json | Out-Null }
        Get-Content -LiteralPath (Join-Path $projectRoot 'events\clicks.jsonl') |
            ForEach-Object { $_ | ConvertFrom-Json | Out-Null }

        $mediaPath = Join-Path $projectRoot $manifest.media.screen
        $inspectionOutput = (& $executable fmp4-inspect $mediaPath 2>&1 | Out-String)
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
        $beforeHash = (Get-FileHash -LiteralPath $mediaPath -Algorithm SHA256).Hash
        $previousErrorActionPreference = $ErrorActionPreference
        try {
            $ErrorActionPreference = 'Continue'
            $failureOutput = (& $executable export-probe $projectRoot $mediaPath 1 2>&1 | Out-String)
            $failureExitCode = $LASTEXITCODE
        }
        finally {
            $ErrorActionPreference = $previousErrorActionPreference
        }
        $afterHash = (Get-FileHash -LiteralPath $mediaPath -Algorithm SHA256).Hash
        if ($failureExitCode -eq 0 -or $beforeHash -ne $afterHash) {
            throw "M4-EXPORT-FAILURE-001 iteration ${iteration}: source-overwrite injection was not atomic"
        }
        $temporaryExport = "$mediaPath.panzo-part.mp4"
        if (Test-Path -LiteralPath $temporaryExport) {
            throw "M4-EXPORT-FAILURE-001 iteration ${iteration}: temporary export remains"
        }

        $results += [pscustomobject]@{
            iteration = $iteration
            disposition = $manifest.state
            durationTick = [int64]$manifest.media.durationTick
            maximumLossSeconds = 2
            sourceHashPreserved = ($beforeHash -eq $afterHash)
            exportFailureObserved = ($failureExitCode -ne 0)
        }
        Write-Host "M4 recovery iteration $iteration/$Iterations passed: $projectRoot"
    }

    $evidencePath = Join-Path $runRoot 'recovery-acceptance.json'
    $results | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $evidencePath -Encoding UTF8
    Write-Host "M4 forced-termination recovery passed $Iterations/$Iterations"
    Write-Host "Evidence: $evidencePath"
}
finally {
    if ($null -ne $child) {
        $child.Refresh()
        if (-not $child.HasExited) {
            Stop-Process -Id $child.Id -Force
            Wait-Process -Id $child.Id -ErrorAction SilentlyContinue
        }
    }
    Pop-Location
}
