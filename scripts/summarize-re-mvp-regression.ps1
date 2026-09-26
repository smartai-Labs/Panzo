param(
    [Parameter(Mandatory=$true)][string]$ReportDirectory
)
$ErrorActionPreference = 'Stop'
$directory = (Resolve-Path -LiteralPath $ReportDirectory).Path
$reportPath = Join-Path $directory 'final-regression.json'
if (-not (Test-Path -LiteralPath $reportPath -PathType Leaf)) { throw 'Wait for the serial run to finish before collecting metrics' }
$report = Get-Content -LiteralPath $reportPath -Raw -Encoding UTF8 | ConvertFrom-Json
function Read-Number([string]$Text,[string]$Pattern) {
    $match = [regex]::Match($Text,$Pattern)
    if ($match.Success) { return [long]$match.Groups[1].Value }
    return $null
}
$cases = foreach ($case in $report.results) {
    $text = Get-Content -LiteralPath $case.stdout -Raw -Encoding UTF8
    $samples = [regex]::Match($text,'P50/P95/P99/max us:\s+([0-9]+)/([0-9]+)/([0-9]+)/([0-9]+)')
    $scrubCount=Read-Number $text 'Scrub request-to-Present samples:\s+([0-9]+)'
    $cadenceCount=Read-Number $text 'Playback cadence samples:\s+([0-9]+)'
    $latency = $null
    if ($samples.Success -and $scrubCount -gt 0) {
        $latency = [ordered]@{p50=[long]$samples.Groups[1].Value;p95=[long]$samples.Groups[2].Value;p99=[long]$samples.Groups[3].Value;max=[long]$samples.Groups[4].Value}
    }
    [ordered]@{
        caseId=$case.caseId;status=$case.status;elapsedMs=$case.elapsedMs;
        peakWorkingSetBytes=$(if ($case.peakWorkingSetBytes -gt 0) { $case.peakWorkingSetBytes } else { $null });
        peakPrivateBytes=$(if ($case.peakPrivateBytes -gt 0) { $case.peakPrivateBytes } else { $null });
        requestToPresentUs=$latency;scrubSamples=$scrubCount;cadenceSamples=$cadenceCount;
        playbackP95Us=$(if ($cadenceCount -gt 0) { Read-Number $text 'Playback Present P95:\s+([0-9]+)' } else { $null });
        playbackP99Us=$(if ($cadenceCount -gt 0) { Read-Number $text 'Playback Present P99:\s+([0-9]+)' } else { $null });
        playbackMaxUs=$(if ($cadenceCount -gt 0) { Read-Number $text 'Playback Present maximum:\s+([0-9]+)' } else { $null });
        distinctScrubFrames=(Read-Number $text 'Scrub distinct source frames:\s+([0-9]+)');
        minimumUpdatesPerTwoSeconds=(Read-Number $text 'minimum distinct updates per 2 seconds:\s+Some\(([0-9]+)\)');
        scrubMaximumGapUs=(Read-Number $text 'Scrub maximum presentation gap:\s+([0-9]+)');
        scrubTimeErrorP95Tick=(Read-Number $text 'Scrub source time error P95 tick:\s+([0-9]+)');
        scrubTimeErrorMaxTick=(Read-Number $text 'Scrub maximum source time error tick:\s+([0-9]+)');
        releasedExactly=$(if ($scrubCount -gt 0) { $text -match 'Scrub release settled exactly:\s+true' } else { $null });
        releaseToInternalPlaybackUs=$(if ($scrubCount -gt 0) { Read-Number $text 'Queued playback latency:\s+([0-9]+)' } else { $null });
        mediaErrors=(Read-Number $text 'Media errors:\s+([0-9]+)');
        proxyPreparationMs=(Read-Number $text 'Preparation elapsed ms:\s+([0-9]+)');
        stdout=$case.stdout;stderr=$case.stderr
    }
}
$fixtureDirectories=@(Get-ChildItem -LiteralPath $report.fixtures -Directory -Filter '*.panzo')
$shortPath=Join-Path $directory 'short-unified.json'
if (Test-Path -LiteralPath $shortPath -PathType Leaf) {
    $short=Get-Content -LiteralPath $shortPath -Raw -Encoding UTF8 | ConvertFrom-Json
    $fixtureDirectories+=@(Get-ChildItem -LiteralPath $short.fixtures -Directory -Filter '*.panzo' | Where-Object { $_.Name -in @('F01.panzo','F02.panzo','F06.panzo') })
}
$fixtures = foreach ($fixture in $fixtureDirectories) {
    $project = Get-Content -LiteralPath (Join-Path $fixture.FullName 'project.json') -Raw -Encoding UTF8 | ConvertFrom-Json
    $path = [IO.Path]::GetFullPath((Join-Path $fixture.FullName $project.media.screen))
    if (-not $path.StartsWith($fixture.FullName + [IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase)) { throw 'Fixture media path leaves the fixture' }
    [ordered]@{name=$fixture.Name;mediaSha256=(Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash;mediaBytes=(Get-Item -LiteralPath $path).Length;manifestDurationTick=$project.media.durationTick;source=$path}
}
[ordered]@{
    runId=$report.runId;executable=$report.executable;executableSha256=$report.sha256;
    sourceReport=$reportPath;cases=@($cases);fixtures=@($fixtures);
    measurementBoundary='Request submission to software Present; not physical input-to-photon. Private/working set sampled every 100 ms. Null means not sampled, never an assumed zero.';
    physicalInputAcceptance='NotRun';visualAcceptance='NotRun';releaseAccepted=$false
} | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath (Join-Path $directory 'metrics-summary.json') -Encoding UTF8
Write-Host "Metrics: $(Join-Path $directory 'metrics-summary.json')"
