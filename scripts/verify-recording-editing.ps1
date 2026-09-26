param(
    [Parameter(Mandatory=$true)][string]$ShortProject,
    [Parameter(Mandatory=$true)][string]$LongProject,
    [Parameter(Mandatory=$true)][string]$Executable,
    [switch]$Extended,
    [string]$ReportDirectory = '',
    [string]$OutputDirectory = ''
)

# Unified automated regression, not a substitute for physical input / visual acceptance.
# Only new fixture directories and child processes created here may be modified/stopped.
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot/validation-common.ps1"
$workspace = [IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot))
$cli = Resolve-PanzoPath $Executable
$runId = 'unified-' + (Get-Date -Format 'yyyyMMdd-HHmmss') + '-' + [guid]::NewGuid().ToString('N').Substring(0,8)
$reportRoot = if ($ReportDirectory) { Resolve-PanzoPath $ReportDirectory } else { Join-Path $workspace "docs/acceptance/RE-MVP/$runId" }
$fixtures = if ($OutputDirectory) { Resolve-PanzoPath $OutputDirectory } else { Join-Path $workspace ".tmp/$runId" }
$null = New-Item -ItemType Directory -Path $reportRoot, $fixtures
$results = [Collections.Generic.List[object]]::new()
$buildHash = $null; $candidate = $null; $runError = ''
$requiredCases = @(Get-PanzoRequiredUnifiedCases -Extended:$Extended)

function Invoke-Case([string]$Name, [string[]]$Arguments, [int]$Timeout = 120) {
    $out = Join-Path $reportRoot "$Name.stdout.txt"
    $err = Join-Path $reportRoot "$Name.stderr.txt"
    $process = Invoke-PanzoProcess -Executable $cli -Arguments $Arguments -WorkingDirectory $workspace -StdoutPath $out -StderrPath $err -TimeoutSeconds $Timeout
    $errorText = $process.errors; $outputText = $process.output
    $ok = -not $process.watchdogExpired -and $process.exitCode -eq 0 -and [string]::IsNullOrWhiteSpace($errorText)
    # Recorder progress is written to stderr, so only explicit errors are failures there.
    if ($Arguments[0] -eq 'window-record-probe') {
        $ok = -not $process.watchdogExpired -and $process.exitCode -eq 0 -and $errorText -notmatch '(?m)^Error:'
    }
    # A missing ExitCode is not proof of success. Require command-specific
    # positive completion evidence, so an empty/crashed process cannot pass.
    $marker = switch ($Arguments[0]) {
        'preview-session-probe' { 'Deterministic start frame:\s+true' }
        'preview-lifecycle-smoke' { 'Open/close 20 / 20: passed' }
        'preview-proxy-probe' { 'Proxy ready:' }
        'recording-editing-probe' { '"sourcePreserved":\s+true' }
        'window-record-probe' { '"framesEncoded":\s+[1-9][0-9]*' }
        default { 'Graceful close:\s+true' }
    }
    if ([string]::IsNullOrWhiteSpace($outputText) -or $outputText -notmatch $marker) { $ok = $false }
    if ($outputText -match 'Media errors:\s+[1-9][0-9]*') { $ok = $false }
    if ($Name -eq 'F03-60-second-playback') {
        foreach ($limit in @(@{label='P95';maximum=25000},@{label='P99';maximum=50000},@{label='maximum';maximum=100000})) {
            $sample = [regex]::Match($outputText, "Playback Present $($limit.label):\s+([0-9]+) us")
            if (-not $sample.Success -or [int64]$sample.Groups[1].Value -gt $limit.maximum) { $ok = $false }
        }
    }
    if ($Name -like 'F03-scrub-*') {
        $updates = [regex]::Match($outputText, 'Scrub minimum distinct updates per 2 seconds:\s+Some\(([0-9]+)\)')
        if (-not $updates.Success -or [int64]$updates.Groups[1].Value -lt 48) { $ok = $false }
    }
    $result = [ordered]@{ caseId=$Name; status=$(if($ok){'Pass'}else{'Fail'}); command=$Arguments; elapsedMs=$process.elapsedMs; watchdogExpired=$process.watchdogExpired; exitCode=$process.exitCode; stdout=$out; stderr=$err }
    $results.Add($result)
    Write-Host "$Name : $($result.status)"
    return (Get-Content -LiteralPath $out -Raw -Encoding UTF8)
}
function Invoke-ScriptCase([string]$Name, [string]$Script, [string[]]$Arguments, [string]$Marker, [int]$Timeout=300) {
    $result = Invoke-PanzoScriptCase -Name $Name -ScriptPath (Join-Path $PSScriptRoot $Script) -Arguments $Arguments -SuccessPattern $Marker -ReportDirectory $reportRoot -WorkingDirectory $workspace -TimeoutSeconds $Timeout
    $results.Add($result)
    Write-Host "$Name : $($result.status)"
}
try {
    $candidate = Get-PanzoCandidate -BuildDirectory (Split-Path -Parent $cli) -Workspace $workspace
    if ((Split-Path -Leaf $cli) -ne 'panzo-cli.exe') { throw 'Executable must be the candidate panzo-cli.exe' }
    $buildHash = (Get-FileHash -LiteralPath $cli).Hash
    foreach ($fixture in @(@{id='F01';source=$ShortProject},@{id='F02';source=$LongProject})) {
        $source = (Resolve-Path -LiteralPath $fixture.source).Path
        $destination = Join-Path $fixtures ($fixture.id + '.panzo')
        Copy-Item -LiteralPath $source -Destination $destination -Recurse
        $null = Invoke-Case "$($fixture.id)-end-seek" @('preview-session-probe', $destination, '9223372036854775807')
        for ($round=1; $round -le 3; $round++) {
            $null = Invoke-Case "$($fixture.id)-scrub-$round" @('preview-scrub-smoke', $destination, '12000')
            $null = Invoke-Case "$($fixture.id)-queued-play-$round" @('preview-queued-playback-smoke', $destination, '2000')
        }
        $null = Invoke-Case "$($fixture.id)-paused" @('preview-paused-smoke', $destination, '5000')
    }
    $f01 = Join-Path $fixtures 'F01.panzo'
    $null = Invoke-Case 'RE-LIFE-01-20-cycles' @('preview-lifecycle-smoke', $f01, '20') 180
    $null = Invoke-Case 'RE-CUT-EXPORT-RENDER' @('recording-editing-probe', $f01, $fixtures) 240
    Invoke-ScriptCase 'RE-SAVE-01-ten-faults' 'verify-editor-atomic.ps1' @('-SourceProject',$f01,'-OutputDirectory',$fixtures,'-ExecutablePath',$cli,'-NoBuild') 'Editor atomic transaction acceptance passed:\s+10/10'

    $stimulus = $null
    try {
        $stimulus = Start-Process -FilePath $cli -WindowStyle Hidden -ArgumentList @('window-stimulus','12','1201','901') -PassThru -RedirectStandardOutput (Join-Path $reportRoot 'F06-stimulus.txt')
        $deadline = [DateTime]::UtcNow.AddSeconds(5)
        do {
            Start-Sleep -Milliseconds 100
            $windows = (& $cli window-list | Out-String)
        } while ($windows -notmatch 'Panzo M4 60 Hz Stability Stimulus' -and [DateTime]::UtcNow -lt $deadline)
        if ($windows -notmatch 'Panzo M4 60 Hz Stability Stimulus') { throw 'F06 stimulus did not open' }
        $f06 = Join-Path $fixtures 'F06.panzo'
        $null = Invoke-Case 'F06-window-recording' @('window-record-probe', $f06, 'Panzo M4 60 Hz Stability Stimulus', '5') 30
        $null = $stimulus.WaitForExit(15000)
        $null = Invoke-Case 'F06-CUT-EXPORT-RENDER' @('recording-editing-probe', $f06, $fixtures) 180
    } finally {
        if ($null -ne $stimulus) {
            $stimulus.Refresh()
            if (-not $stimulus.HasExited) { Stop-Process -Id $stimulus.Id -Force }
        }
    }
    Invoke-ScriptCase 'RE-REC-native-controller' 'verify-recorder-ui.ps1' @('-ExecutablePath',$cli,'-NoBuild') 'Recorder Window record/export/reopen acceptance passed:'
    if ($Extended) {
        $f03Directory = Join-Path $fixtures 'F03'
        Invoke-ScriptCase 'F03-60-second-recording' 'verify-m4-stability.ps1' @('-DurationSeconds','60','-OutputDirectory',$f03Directory,'-ExecutablePath',$cli,'-NoBuild') 'M4 stability acceptance passed:' 240
        $f03 = Get-ChildItem -LiteralPath $f03Directory -Directory -Filter '*.panzo' | Select-Object -First 1
        if ($null -ne $f03) {
            $null = Invoke-Case 'F03-60-second-playback' @('preview-smoke', $f03.FullName, '60000') 120
            $null = Invoke-Case 'F03-proxy-preparation' @('preview-proxy-probe', (Join-Path $f03.FullName 'media/screen.mp4')) 180
            for ($round=1; $round -le 3; $round++) { $null = Invoke-Case "F03-scrub-$round" @('preview-scrub-smoke', $f03.FullName, '12000') }
        }
        $f05Directory = Join-Path $fixtures 'F05'
        Invoke-ScriptCase 'RE-STABLE-01-30-minutes' 'verify-m4-stability.ps1' @('-DurationSeconds','1800','-OutputDirectory',$f05Directory,'-ExecutablePath',$cli,'-NoBuild') 'M4 stability acceptance passed:' 2100
        Invoke-ScriptCase 'RE-STABLE-01-five-recoveries' 'verify-m4-recovery.ps1' @('-OutputDirectory',$fixtures,'-ExecutablePath',$cli,'-NoBuild') 'M4 forced-termination recovery passed 5/5' 600
    }
} catch {
    $runError = $_.Exception.Message
} finally {
    $environment = $null
    try {
        $environment = [ordered]@{
            os=(Get-CimInstance Win32_OperatingSystem | Select-Object Caption,Version,BuildNumber);
            cpu=@(Get-CimInstance Win32_Processor | Select-Object Name);
            gpu=@(Get-CimInstance Win32_VideoController | Select-Object Name,DriverVersion,CurrentHorizontalResolution,CurrentVerticalResolution,CurrentRefreshRate);
            physicalMemoryBytes=(Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory
        }
    } catch { $environment = @{error=$_.Exception.Message} }
    $report = [ordered]@{
        runId=$runId; executable=$cli; executableSha256=$buildHash; capturedAtUtc=[DateTime]::UtcNow.ToString('o');
        scope='Automated regression only. Not physical input-to-photon, full DPI screenshots, or human GUI workflow acceptance.';
        fixtures=$fixtures; results=$results.ToArray();
        environment=$environment;
        manualAcceptance='NotRun'; releaseAccepted=$false;
        deferred=@('audio','cross-GPU machines','automatic camera naturalness');
        reportType='unified-regression'; extended=[bool]$Extended
    }
    $completed = Complete-PanzoRegressionReport -Path (Join-Path $reportRoot 'automated-regression.json') -Report $report -RequiredCases $requiredCases -Candidate $candidate -Workspace $workspace -RunError $runError
    Write-Host "Unified automated evidence: $reportRoot"
}
if ($completed.status -ne 'Pass') { throw "Unified regression failed: $($completed.failures -join '; ')" }
