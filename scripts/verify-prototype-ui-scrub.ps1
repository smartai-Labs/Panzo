param(
    [Parameter(Mandatory=$true)][string]$Executable,
    [string]$ProjectRoot='.tmp/final-ui-20260906-142316/F05.panzo',
    [ValidateRange(1,20)][int]$Rounds=5
)
$ErrorActionPreference='Stop'
$cli=(Resolve-Path -LiteralPath $Executable).Path
$project=(Resolve-Path -LiteralPath $ProjectRoot).Path
$sha=(Get-FileHash -LiteralPath $cli).Hash
$reportDirectory=Join-Path (Split-Path -Parent $PSScriptRoot) ('docs/acceptance/RE-MVP/ui-scrub-diagnostic-'+(Get-Date -Format 'yyyyMMdd-HHmmss'))
$null=New-Item -ItemType Directory -Path $reportDirectory
$results=[Collections.Generic.List[object]]::new()
for($round=1;$round -le $Rounds;$round++) {
    $stdout=Join-Path $reportDirectory "round-$round.stdout.txt"
    $stderr=Join-Path $reportDirectory "round-$round.stderr.txt"
    if($project.Contains('"')){throw 'Invalid path'}
    $child=Start-Process -FilePath $cli -WindowStyle Hidden -ArgumentList ('preview-scrub-smoke "'+$project+'" 12000') -PassThru -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    $expired=-not $child.WaitForExit(45000)
    if($expired){Stop-Process -Id $child.Id -Force;$null=$child.WaitForExit(5000)}
    $child.Refresh()
    $output=[string](Get-Content -LiteralPath $stdout -Raw -Encoding UTF8)
    $errors=[string](Get-Content -LiteralPath $stderr -Raw -Encoding UTF8)
    $ok=-not $expired -and ($null -eq $child.ExitCode -or $child.ExitCode -eq 0) -and $output -match 'Media errors:\s+0' -and $output -match 'Scrub release settled exactly:\s+true' -and $output -match 'Graceful close:\s+true' -and $errors -notmatch '(?m)^Error:'
    $results.Add([ordered]@{round=$round;status=$(if($ok){'Pass'}else{'Fail'});watchdogExpired=$expired;stdout=$stdout;stderr=$stderr;exitCode=$child.ExitCode})
    Write-Host "F05 diagnostic round $round : $($results[$results.Count-1].status)"
}
[ordered]@{executable=$cli;sha256=$sha;unchanged=((Get-FileHash -LiteralPath $cli).Hash -eq $sha);project=$project;scope='Targeted software scrub reproduction; does not erase an earlier failure or establish a root cause by itself';results=$results.ToArray()} | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $reportDirectory 'scrub-diagnostic.json') -Encoding UTF8
Write-Host "Evidence: $reportDirectory"
