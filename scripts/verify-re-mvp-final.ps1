param(
    [Parameter(Mandatory=$true)][string]$FixtureRoot,
    [Parameter(Mandatory=$true)][string]$Executable,
    [string]$WaitForReport = '',
    [string]$PreparedLongProject = '',
    [string]$ReportDirectory = '',
    [string]$OutputDirectory = ''
)
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot/validation-common.ps1"
$workspace = [IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot))
$cli = Resolve-PanzoPath $Executable
$source = Resolve-PanzoPath $FixtureRoot
$runId = 'final-ui-' + (Get-Date -Format 'yyyyMMdd-HHmmss') + '-' + [guid]::NewGuid().ToString('N').Substring(0,8)
$evidence = if ($ReportDirectory) { Resolve-PanzoPath $ReportDirectory } else { Join-Path $workspace "docs/acceptance/RE-MVP/$runId" }
$copies = if ($OutputDirectory) { Resolve-PanzoPath $OutputDirectory } else { Join-Path $workspace ".tmp/$runId" }
$null = New-Item -ItemType Directory -Path $evidence,$copies
$hash = $null; $candidate = $null; $runError = ''
$requiredCases = @(Get-PanzoRequiredFinalCases -PreparedLongProject:([bool]$PreparedLongProject))
$results = [Collections.Generic.List[object]]::new()
function Copy-Fixture([string]$Original,[string]$Name) {
    $destination = Join-Path $copies "$Name.panzo"
    $null = New-Item -ItemType Directory -Path $destination
    # Cold means new process + no application cache. Source files are copied, never deleted.
    foreach ($entry in Get-ChildItem -LiteralPath $Original) {
        if ($entry.Name -notin @('cache','diagnostics','recording.lock','.editor-write.lock')) {
            Copy-Item -LiteralPath $entry.FullName -Destination (Join-Path $destination $entry.Name) -Recurse
        }
    }
    return $destination
}
function Probe([string]$Name,[string[]]$Arguments,[int]$Timeout=120) {
    $out = Join-Path $evidence "$Name.stdout.txt"
    $err = Join-Path $evidence "$Name.stderr.txt"
    $child = Invoke-PanzoProcess -Executable $cli -Arguments $Arguments -WorkingDirectory $workspace -StdoutPath $out -StderrPath $err -TimeoutSeconds $Timeout
    $text=$child.output; $errorText=$child.errors
    $ok=-not $child.watchdogExpired -and $child.exitCode -eq 0 -and $errorText -notmatch '(?m)^Error:' -and $text -notmatch 'Media errors:\s+[1-9]'
    $marker = switch ($Arguments[0]) {
        'preview-session-probe' { 'Deterministic start frame:\s+true' }
        'preview-proxy-probe' { 'Proxy ready:' }
        default { 'Graceful close:\s+true' }
    }
    if (-not $text -or $text -notmatch $marker) { $ok=$false }
    if ($Name -like '*60s-play*') {
        foreach ($limit in @(@{key='P95';value=25000},@{key='P99';value=50000},@{key='maximum';value=100000})) {
            $sample=[regex]::Match($text,"Playback Present $($limit.key):\s+([0-9]+) us")
            if (-not $sample.Success -or [long]$sample.Groups[1].Value -gt $limit.value) { $ok=$false }
        }
    }
    if ($Name -like 'F03-scrub-*') {
        $sample=[regex]::Match($text,'minimum distinct updates per 2 seconds:\s+Some\(([0-9]+)\)')
        if (-not $sample.Success -or [int]$sample.Groups[1].Value -lt 48) { $ok=$false }
    }
    $results.Add([ordered]@{caseId=$Name;status=$(if($ok){'Pass'}else{'Fail'});arguments=$Arguments;elapsedMs=$child.elapsedMs;peakWorkingSetBytes=$child.peakWorkingSetBytes;peakPrivateBytes=$child.peakPrivateBytes;watchdogExpired=$child.watchdogExpired;exitCode=$child.exitCode;stdout=$out;stderr=$err})
    Write-Host "$Name : $($results[$results.Count-1].status)"
}
function Cargo-Check([string]$Name,[string[]]$Arguments) {
    $child = Invoke-PanzoProcess -Executable (Get-Command cargo -CommandType Application).Source -Arguments $Arguments -WorkingDirectory $workspace -StdoutPath (Join-Path $evidence "$Name.log") -StderrPath (Join-Path $evidence "$Name.stderr.log") -TimeoutSeconds 300
    $ok = -not $child.watchdogExpired -and $child.exitCode -eq 0
    $results.Add([ordered]@{caseId=$Name;status=$(if($ok){'Pass'}else{'Fail'});exitCode=$child.exitCode;watchdogExpired=$child.watchdogExpired})
    return $child.exitCode
}
try {
    $candidate = Get-PanzoCandidate -BuildDirectory (Split-Path -Parent $cli) -Workspace $workspace
    if ((Split-Path -Leaf $cli) -ne 'panzo-cli.exe') { throw 'Executable must be the candidate panzo-cli.exe' }
    $hash = (Get-FileHash -LiteralPath $cli).Hash
    # Keep GPU tests serial with the existing run. Waiting failures are also reported.
    if ($WaitForReport) {
        $waitPath = Resolve-PanzoPath $WaitForReport
        $deadline = [DateTime]::UtcNow.AddHours(1)
        while (-not (Test-Path -LiteralPath $waitPath -PathType Leaf)) {
            if ([DateTime]::UtcNow -ge $deadline) { throw 'Previous validation report did not arrive within one hour' }
            Start-Sleep -Seconds 1
        }
    }
    $nestedDirectory = Join-Path $evidence 'short-unified'
    $nestedReport = Join-Path $nestedDirectory 'automated-regression.json'
    $nested = Invoke-PanzoScriptCase -Name 'short-unified' -ScriptPath "$PSScriptRoot/verify-recording-editing.ps1" -Arguments @('-ShortProject',(Join-Path $source 'F01.panzo'),'-LongProject',(Join-Path $source 'F02.panzo'),'-Executable',$cli,'-ReportDirectory',$nestedDirectory,'-OutputDirectory',(Join-Path $copies 'short-unified')) -SuccessPattern 'Unified automated evidence:' -ReportDirectory $evidence -WorkingDirectory $workspace -TimeoutSeconds 1800 -ExpectedReport $nestedReport -Candidate $candidate -RequiredCases @(Get-PanzoRequiredUnifiedCases)
    $results.Add($nested)
    if (Test-Path -LiteralPath $nestedReport) { Copy-Item -LiteralPath $nestedReport -Destination (Join-Path $evidence 'short-unified.json') }
    $f03Original=(Get-ChildItem -LiteralPath (Join-Path $source 'F03') -Directory -Filter '*.panzo' | Select-Object -First 1).FullName
    $f03=Copy-Fixture $f03Original 'F03-cold'
    Probe 'F03-cold-60s-play' @('preview-smoke',$f03,'60000') 120
    Probe 'F03-first-proxy-preparation' @('preview-proxy-probe',(Join-Path $f03 'media/screen.mp4')) 240
    Probe 'F03-warm-60s-play' @('preview-smoke',$f03,'60000') 120
    for ($round=1;$round -le 3;$round++) { Probe "F03-scrub-$round" @('preview-scrub-smoke',$f03,'12000') }
    $f05Original=(Get-ChildItem -LiteralPath (Join-Path $source 'F05') -Directory -Filter '*.panzo' | Select-Object -First 1).FullName
    $f05=Copy-Fixture $f05Original 'F05'
    if ($PreparedLongProject) {
        $prepared=(Resolve-Path -LiteralPath $PreparedLongProject).Path
        if ((Get-FileHash -LiteralPath (Join-Path $prepared 'media/screen.mp4')).Hash -ne (Get-FileHash -LiteralPath (Join-Path $f05 'media/screen.mp4')).Hash) { throw 'Prepared cache does not belong to this source' }
        Copy-Item -LiteralPath (Join-Path $prepared 'cache') -Destination (Join-Path $f05 'cache') -Recurse
        Probe 'F05-cached-end-seek' @('preview-session-probe',$f05,'9223372036854775807') 180
        Probe 'F05-existing-proxy-open' @('preview-proxy-probe',(Join-Path $f05 'media/screen.mp4')) 900
    } else {
        Probe 'F05-cold-end-seek' @('preview-session-probe',$f05,'9223372036854775807') 180
        Probe 'F05-first-proxy-preparation' @('preview-proxy-probe',(Join-Path $f05 'media/screen.mp4')) 900
    }
    for ($round=1;$round -le 3;$round++) { Probe "F05-scrub-$round" @('preview-scrub-smoke',$f05,'12000') }
    Probe 'F05-five-minute-scrub-resources' @('preview-scrub-smoke',$f05,'302000') 360
    $target = Split-Path -Parent $candidate.directory
    $fmtExit=Cargo-Check 'fmt' @('fmt','--all','--','--check')
    $clippyExit=Cargo-Check 'clippy' @('clippy','--locked','--offline','--workspace','--all-targets','--target-dir',$target,'--','-D','warnings')
    $testsExit=Cargo-Check 'unit-tests' @('test','--locked','--offline','--workspace','--all-targets','--target-dir',$target,'--','--test-threads=1')
    [ordered]@{fmt=$fmtExit;clippy=$clippyExit;tests=$testsExit} | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $evidence 'quality-exit-codes.json') -Encoding UTF8
    if ($fmtExit -ne 0 -or $clippyExit -ne 0 -or $testsExit -ne 0) { throw 'Quality checks failed; inspect the saved exit codes and logs' }
} catch {
    $runError=$_.Exception.Message
} finally {
    $report=[ordered]@{reportType='final-regression';runId=$runId;executable=$cli;sha256=$hash;results=$results.ToArray();fixtures=$copies;preparedLongProject=[bool]$PreparedLongProject;nestedReports=@('short-unified/automated-regression.json');manualAcceptance='NotRun';releaseAccepted=$false;scope='CLI/native-handler regression, not physical input-to-photon or user visual acceptance'}
    $completed = Complete-PanzoRegressionReport -Path (Join-Path $evidence 'final-regression.json') -Report $report -RequiredCases $requiredCases -Candidate $candidate -Workspace $workspace -RunError $runError
    Write-Host "Final regression: $evidence"
}
if ($completed.status -ne 'Pass') { throw "Final regression failed: $($completed.failures -join '; ')" }
