param(
    [Parameter(Mandatory=$true)][string]$Mode,
    [Parameter(Mandatory=$true)][string]$Workspace,
    [Parameter(Mandatory=$true)][string]$BuildDirectory,
    [Parameter(Mandatory=$true)][string]$ReportPath,
    [string]$Echo = ''
)
$ErrorActionPreference = 'Stop'
. (Join-Path (Split-Path -Parent $PSScriptRoot) 'validation-common.ps1')
if ($Mode -eq 'exit17') { Write-Output 'Fixture completed'; exit 17 }
if ($Mode -eq 'missing-report') { Write-Output 'Fixture completed'; return }
if ($Mode -eq 'empty') { return }
if ($Mode -eq 'timeout') { Start-Sleep -Seconds 20; return }
if ($Mode -eq 'timeout-descendant') {
    $child = Start-Process -FilePath (Join-Path $PSHOME 'powershell.exe') -WindowStyle Hidden -ArgumentList @('-NoProfile','-NonInteractive','-Command','Start-Sleep -Seconds 20') -PassThru
    $child.Id | Set-Content -LiteralPath "$ReportPath.pid" -Encoding UTF8
    Start-Sleep -Seconds 20
    return
}
$candidate = Get-PanzoCandidate -BuildDirectory $BuildDirectory -Workspace $Workspace
$required = @('fixture')
$caseResults = @(); $runError = ''
try {
    if ($Mode -eq 'throw') { throw 'Deliberate fixture exception' }
    if ($Mode -eq 'expected-native') {
        & (Join-Path $PSHOME 'powershell.exe') -NoProfile -NonInteractive -Command 'exit 86'
        if ($LASTEXITCODE -ne 86) { throw 'Expected fault injection did not occur' }
    }
    if ($Mode -eq 'argument' -and $Echo -cne 'a "quoted" tail\') { throw "Argument quoting changed: $Echo" }
    if ($Mode -eq 'missing-case') { $required += 'never-executed' }
    $caseResults = @([ordered]@{caseId='fixture';status=$(if($Mode -eq 'child-fail'){'Fail'}else{'Pass'})})
} catch { $runError = $_.Exception.Message }
finally {
    $report = [ordered]@{reportType='fixture-regression';results=$caseResults}
    $completed = Complete-PanzoRegressionReport -Path $ReportPath -Report $report -RequiredCases $required -Candidate $candidate -Workspace $Workspace -RunError $runError
}
# child-fail deliberately returns zero so the caller must inspect the JSON too.
if ($completed.status -ne 'Pass' -and $Mode -ne 'child-fail') { throw "Fixture regression failed: $($completed.failures -join '; ')" }
Write-Output 'Fixture completed'
