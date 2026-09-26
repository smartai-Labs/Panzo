param(
    [string]$ShortProject = 'C:\Users\Administrator\Videos\Panzo\panzo-1788359585144-000.panzo',
    [string]$LongProject = '.tmp\m1-acceptance-49a75b0350cc4e79a2e31fbdfce3b618.panzo',
    [string]$Executable = 'target\release\panzo-cli.exe',
    [ValidateRange(1, 20)][int]$Repeats = 3,
    [ValidateRange(10, 120)][int]$WatchdogSeconds = 30,
    [ValidateSet('r0', 'r1', 're-mvp')][string]$Phase = 'r0'
)

# R0 characterization, not final RE-PERF acceptance. Never edits the supplied projects.
$ErrorActionPreference = 'Stop'
$workspaceRoot = [IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot))
$runId = $Phase + '-' + (Get-Date -Format 'yyyyMMdd-HHmmss') + '-' + [guid]::NewGuid().ToString('N').Substring(0, 8)
$runRoot = Join-Path $workspaceRoot "docs\acceptance\RE-MVP\$runId"
$fixtureRoot = Join-Path $workspaceRoot ".tmp\$runId"
$null = New-Item -ItemType Directory -Path $runRoot, $fixtureRoot

function Resolve-WorkspacePath([string]$Path) {
    if ([IO.Path]::IsPathRooted($Path)) { return (Resolve-Path -LiteralPath $Path).Path }
    return (Resolve-Path -LiteralPath (Join-Path $workspaceRoot $Path)).Path
}
function Write-ReportJson($Value, [string]$Path) {
    $Value | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $Path -Encoding UTF8
}
function Get-AssetHashes([string]$Project) {
    $hashes = [ordered]@{}
    foreach ($relative in @('project.json', 'media\screen.mp4', 'events\cursor.jsonl', 'events\clicks.jsonl', 'tracks\camera.json')) {
        $path = Join-Path $Project $relative
        if (Test-Path -LiteralPath $path) { $hashes[$relative] = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash }
    }
    return $hashes
}
function Invoke-Probe([string]$CaseName, [string[]]$Arguments) {
    $stdout = Join-Path $runRoot "$CaseName.stdout.txt"
    $stderr = Join-Path $runRoot "$CaseName.stderr.txt"
    # Each argument is quoted independently. Paths with embedded quotes are rejected.
    foreach ($argument in $Arguments) { if ($argument.Contains('"')) { throw 'Quotes are not supported in probe arguments' } }
    $quoted = ($Arguments | ForEach-Object { '"' + $_ + '"' }) -join ' '
    $timer = [Diagnostics.Stopwatch]::StartNew()
    $start = New-Object Diagnostics.ProcessStartInfo
    $start.FileName = $executablePath
    $start.Arguments = $quoted
    $start.WorkingDirectory = $workspaceRoot
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $process = New-Object Diagnostics.Process
    $process.StartInfo = $start
    $null = $process.Start()
    $outputTask = $process.StandardOutput.ReadToEndAsync()
    $errorTask = $process.StandardError.ReadToEndAsync()
    $timedOut = -not $process.WaitForExit($WatchdogSeconds * 1000)
    if ($timedOut) {
        # Only the exact child started above can be terminated; unrelated Panzo windows are untouched.
        $process.Kill()
        $null = $process.WaitForExit(5000)
    }
    $outputReady = $outputTask.Wait(5000)
    $errorReady = $errorTask.Wait(5000)
    $text = if ($outputReady) { $outputTask.Result } else { '' }
    $errorText = if ($errorReady) { $errorTask.Result } else { 'Timed out reading probe output' }
    [IO.File]::WriteAllText($stdout, $text, (New-Object Text.UTF8Encoding $false))
    [IO.File]::WriteAllText($stderr, $errorText, (New-Object Text.UTF8Encoding $false))
    $probeResult = [pscustomobject]@{
        caseId = $CaseName; command = $Arguments[0]; status = $(if ($timedOut -or -not $outputReady -or -not $errorReady -or $process.ExitCode -ne 0) { 'Fail' } else { 'Pass' })
        exitCode = $process.ExitCode; watchdogExpired = $timedOut; processElapsedMs = $timer.ElapsedMilliseconds
        output = $text; error = $errorText; stdoutFile = "$CaseName.stdout.txt"; stderrFile = "$CaseName.stderr.txt"
    }
    Write-ReportJson $probeResult (Join-Path $runRoot "$CaseName.result.json")
    return $probeResult
}
function Get-Percentile([double[]]$Values, [double]$Percentile) {
    if (-not $Values.Count) { return $null }
    $sorted = @($Values | Sort-Object)
    return $sorted[[Math]::Max(0, [Math]::Ceiling($sorted.Count * $Percentile) - 1)]
}

$executablePath = Resolve-WorkspacePath $Executable
$environmentInfo = [ordered]@{
    runId = $runId; capturedAtUtc = [DateTime]::UtcNow.ToString('o')
    executable = $executablePath; executableSha256 = (Get-FileHash -LiteralPath $executablePath).Hash
    executableModifiedUtc = (Get-Item -LiteralPath $executablePath).LastWriteTimeUtc.ToString('o')
    buildType = 'release'; phase = $Phase; runtime = [Environment]::Version.ToString()
    externalGpuLoadControlled = $false
    note = 'GUI latency is internal seek-to-playing-state, not input-to-visible-video. R1 adds scripted native input-handler/Present regression, not physical pointer-to-DWM sampling.'
}
try {
    $environmentInfo.os = Get-CimInstance Win32_OperatingSystem | Select-Object Caption, Version, BuildNumber
    $environmentInfo.cpu = @(Get-CimInstance Win32_Processor | Select-Object Name, NumberOfCores, NumberOfLogicalProcessors)
    $environmentInfo.gpu = @(Get-CimInstance Win32_VideoController | Select-Object Name, DriverVersion, CurrentHorizontalResolution, CurrentVerticalResolution, CurrentRefreshRate)
    $environmentInfo.physicalMemoryBytes = (Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory
} catch { $environmentInfo.environmentCaptureError = $_.Exception.Message }
Write-ReportJson $environmentInfo (Join-Path $runRoot 'environment.json')

$fixtures = @(); $results = @()
foreach ($entry in @(@{id='F01'; path=$ShortProject}, @{id='F02'; path=$LongProject})) {
    try { $source = Resolve-WorkspacePath $entry.path } catch {
        $fixtures += [pscustomobject]@{id=$entry.id; status='NotRun'; reason=$_.Exception.Message}; continue
    }
    if (-not (Test-Path -LiteralPath (Join-Path $source 'project.json'))) { throw 'Missing project.json' }
    $manifest = Get-Content -LiteralPath (Join-Path $source 'project.json') -Encoding UTF8 -Raw | ConvertFrom-Json
    $before = Get-AssetHashes $source
    $copy = Join-Path $fixtureRoot ($entry.id + '.panzo')
    Copy-Item -LiteralPath $source -Destination $copy -Recurse
    Write-Host "$Phase $($entry.id): protected copy ready; measuring $($manifest.media.durationTick) ticks"
    $fixtureResults = @()
    try {
        $session = Invoke-Probe "$($entry.id)-end-seek" @('preview-session-probe', $copy, [string]$manifest.media.durationTick)
        $session | Add-Member -NotePropertyName metricKind -NotePropertyValue 'end-seek-correctness'
        $fixtureResults += $session
        $latencies = @()
        for ($iteration = 1; $iteration -le $Repeats; $iteration++) {
            $result = Invoke-Probe "$($entry.id)-queued-$iteration" @('preview-queued-playback-smoke', $copy, '6000')
            $match = [regex]::Match([string]$result.output, 'Queued playback latency:\s+(\d+)\s+us')
            $latency = if ($match.Success) { [double]$match.Groups[1].Value / 1000 } else { $null }
            $result | Add-Member -NotePropertyName metricKind -NotePropertyValue 'internal-queued-playback-state'
            $result | Add-Member -NotePropertyName latencyMs -NotePropertyValue $latency
            $result | Add-Member -NotePropertyName cacheState -NotePropertyValue 'fresh-process-os-cache-uncontrolled'
            if ($null -eq $latency -or $result.output -notmatch 'Queued playback completed:\s+true') { $result.status = 'Fail' }
            elseif ($latency -gt 750) { $result.status = 'Fail' }
            if ($null -ne $latency) { $latencies += $latency }
            $fixtureResults += $result
            Write-Host "  queued $iteration/$Repeats : $($result.status), $latency ms (internal metric)"
        }
        $trim = Invoke-Probe "$($entry.id)-trim" @('preview-trim-smoke', $copy, '6000')
        $trim | Add-Member -NotePropertyName metricKind -NotePropertyValue 'camera-edit-ui-commit'
        $fixtureResults += $trim
        if ($Phase -ne 'r0') {
            $sourceMedia = Join-Path $copy 'media\screen.mp4'
            $middleMs = [Math]::Floor([double]$manifest.media.durationTick / 20000)
            $endMs = [Math]::Ceiling([double]$manifest.media.durationTick / 10000) + 1
            foreach ($tickMs in @(0, $middleMs, $endMs)) {
                $seek = Invoke-Probe "$($entry.id)-pixel-seek-$tickMs" @('decode-seek-probe', $sourceMedia, [string]$tickMs)
                if ($seek.output -notmatch 'Pixels match sequential decode:\s+true') { $seek.status = 'Fail' }
                $fixtureResults += $seek
            }
            for ($dragRun = 1; $dragRun -le $Repeats; $dragRun++) {
                $scrub = Invoke-Probe "$($entry.id)-scrub-$dragRun" @('preview-scrub-smoke', $copy, '5000')
                $gap = [regex]::Match([string]$scrub.output, 'Scrub maximum presentation gap:\s+(\d+)\s+us')
                if (-not $gap.Success -or [double]$gap.Groups[1].Value -gt 150000) { $scrub.status = 'Fail' }
                $fixtureResults += $scrub
                Write-Host "  scrub $dragRun/$Repeats : $($scrub.status), maximum gap $($gap.Groups[1].Value) us"
            }
            $fixtureResults += Invoke-Probe "$($entry.id)-reopen" @('preview-reopen-smoke', $copy, '300')
            $hash = Invoke-Probe "$($entry.id)-render-hash" @('render-hash-probe', $copy, [string]$middleMs)
            if ($hash.output -notmatch 'Hashes match:\s+true') { $hash.status = 'Fail' }
            $fixtureResults += $hash
            $exportPath = Join-Path $fixtureRoot "$($entry.id)-export.mp4"
            $fixtureResults += Invoke-Probe "$($entry.id)-export" @('export-probe', $copy, $exportPath, '2')
            $decode = Invoke-Probe "$($entry.id)-export-decode" @('decode-probe', $exportPath, '121')
            if ($decode.output -notmatch 'Decoded frames:\s+120\b' -or $decode.output -notmatch 'Strictly increasing PTS:\s+true') { $decode.status = 'Fail' }
            $fixtureResults += $decode
            if ($entry.id -eq 'F01') {
                $play = Invoke-Probe 'F01-play-through-end' @('preview-smoke', $copy, [string]($endMs + 1000))
                if ($play.output -notmatch "Last project tick:\s+$($manifest.media.durationTick)\b") { $play.status = 'Fail' }
                $fixtureResults += $play
            }
            foreach ($probe in $fixtureResults) {
                if ($probe.output -match 'Media errors:\s+[1-9]') { $probe.status = 'Fail' }
                Write-ReportJson $probe (Join-Path $runRoot "$($probe.caseId).result.json")
            }
            Write-Host "  R1 native regressions: $(@($fixtureResults | Where-Object status -eq 'Pass').Count)/$($fixtureResults.Count) passed"
        }
        $fixtures += [pscustomobject]@{
            id=$entry.id; sourceProject=$source; protectedCopy=$copy; status='Measured'
            durationTick=$manifest.media.durationTick; captureSize=$manifest.capture.contentSizePx
            hashes=$before; samples=$latencies.Count; queuedMedianMs=(Get-Percentile $latencies .5)
            queuedP95Ms=(Get-Percentile $latencies .95); queuedMaxMs=(Get-Percentile $latencies 1)
        }
    } finally {
        Write-Host "  $($entry.id): verifying original asset hashes"
        $after = Get-AssetHashes $source
        if (($before | ConvertTo-Json -Compress) -ne ($after | ConvertTo-Json -Compress)) { throw "Source fixture changed unexpectedly: $source" }
        $results += $fixtureResults
        Write-Host "  $($entry.id): writing baseline summary"
        Write-ReportJson @{runId=$runId; fixtures=$fixtures; measurements=$results; sourceAssetsPreserved=$true; releaseAcceptance='NotRun'} (Join-Path $runRoot 'baseline.json')
    }
}
Write-Host "$Phase measurements recorded: $runRoot"
Write-Host 'Full RE-PERF, physical input-to-visible-frame and final visual acceptance remain NotRun.'
if ($Phase -ne 'r0' -and (@($fixtures | Where-Object status -eq 'Measured').Count -ne 2 -or @($results | Where-Object status -ne 'Pass').Count -ne 0)) {
    throw "R1 preview regression failed; see $runRoot\baseline.json"
}
