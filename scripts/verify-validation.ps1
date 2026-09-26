param([string]$OutputDirectory = '')
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot/validation-common.ps1"
$workspace = Resolve-PanzoPath (Split-Path -Parent $PSScriptRoot)
$unicode = -join ([char[]](0x4E2D,0x6587))
$runRoot = if ($OutputDirectory) { Resolve-PanzoPath $OutputDirectory } else { Join-Path $workspace ('.tmp/validation-' + [guid]::NewGuid().ToString('N') + " $unicode space") }
$fixtureWorkspace = Join-Path $runRoot 'source'
$fixtureBuild = Join-Path $runRoot 'candidate release'
$null = New-Item -ItemType Directory -Path $runRoot,$fixtureWorkspace,$fixtureBuild,(Join-Path $fixtureWorkspace 'crates')
foreach ($name in @('Cargo.toml','Cargo.lock','rust-toolchain.toml','rustfmt.toml','crates/fixture.rs')) {
    Set-Content -LiteralPath (Join-Path $fixtureWorkspace $name) -Value 'fixture-source' -Encoding UTF8
}
foreach ($name in @('panzo.exe','panzo-cli.exe')) { Set-Content -LiteralPath (Join-Path $fixtureBuild $name) -Value "fixture-$name" -Encoding UTF8 }
$identity = Get-PanzoSourceIdentity $fixtureWorkspace
[ordered]@{schemaVersion=1;configuration='release';source=$identity;binaries=@(Get-PanzoBinaryIdentity $fixtureBuild)} | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $fixtureBuild 'candidate-build.json') -Encoding UTF8
$candidate = Get-PanzoCandidate -BuildDirectory $fixtureBuild -Workspace $fixtureWorkspace
$checks = [Collections.Generic.List[object]]::new()
function Check([string]$Name, [scriptblock]$Body) {
    try { & $Body; $checks.Add([ordered]@{caseId=$Name;status='Pass'}); Write-Host "PASS $Name" }
    catch { $checks.Add([ordered]@{caseId=$Name;status='Fail';error=$_.Exception.Message}); Write-Host "FAIL $Name : $($_.Exception.Message)" }
}
function Assert([bool]$Condition, [string]$Message) { if (-not $Condition) { throw $Message } }

try {
    foreach ($mode in @('success','throw','exit17','expected-native','missing-report','child-fail','missing-case','empty','timeout','timeout-descendant','argument')) {
        Check $mode {
            $reportPath = Join-Path $runRoot "$mode.json"
            $arguments = @('-Mode',$mode,'-Workspace',$fixtureWorkspace,'-BuildDirectory',$fixtureBuild,'-ReportPath',$reportPath)
            if ($mode -eq 'argument') { $arguments += @('-Echo','a "quoted" tail\') }
            $timeout = if ($mode -eq 'timeout') { 1 } elseif ($mode -eq 'timeout-descendant') { 3 } else { 30 }
            $result = Invoke-PanzoScriptCase -Name $mode -ScriptPath "$PSScriptRoot/fixtures/validation-case.ps1" -Arguments $arguments -SuccessPattern 'Fixture completed' -ReportDirectory $runRoot -WorkingDirectory $runRoot -ExpectedReport $reportPath -Candidate $candidate -RequiredCases @('fixture') -TimeoutSeconds $timeout
            $expected = if ($mode -in @('success','expected-native','argument')) { 'Pass' } else { 'Fail' }
            Assert ($result.status -eq $expected) "Expected $expected, got $($result.status): $($result.error)"
            if ($mode -eq 'exit17') { Assert ($result.exitCode -eq 17) 'exit 17 was swallowed' }
            if ($mode -eq 'expected-native') { Assert ($result.exitCode -eq 0) 'Expected native failure leaked to script status' }
            if ($mode -eq 'child-fail') { Assert ($result.exitCode -eq 0) 'Fixture must prove zero exit plus structured Fail is rejected' }
            if ($mode -eq 'timeout-descendant') {
                $childId = [int](Get-Content -LiteralPath "$reportPath.pid" -Encoding UTF8)
                Assert ($null -eq (Get-Process -Id $childId -ErrorAction SilentlyContinue)) 'Timed-out script left its owned native child running'
            }
            if ($mode -in @('throw','missing-case')) {
                Assert (Test-Path -LiteralPath $reportPath) 'finally did not preserve the failure report'
                $saved = Get-Content -LiteralPath $reportPath -Raw -Encoding UTF8 | ConvertFrom-Json
                Assert ($saved.status -eq 'Fail') 'Failure report says Pass'
                if ($mode -eq 'missing-case') { Assert (@($saved.results | Where-Object status -eq 'NotRun').Count -eq 1) 'Missing case was not recorded as NotRun' }
            }
        }
    }

    Check 'binary-identity-both-files' {
        foreach ($name in @('panzo.exe','panzo-cli.exe')) {
            $path = Join-Path $fixtureBuild $name
            $original = [IO.File]::ReadAllBytes($path)
            try {
                [IO.File]::AppendAllText($path, 'changed')
                $rejected = $false
                try { $null = Get-PanzoCandidate -BuildDirectory $fixtureBuild -Workspace $fixtureWorkspace } catch { $rejected = $true }
                Assert $rejected "$name changed without invalidating candidate"
            } finally { [IO.File]::WriteAllBytes($path,$original) }
        }
    }
    Check 'source-identity' {
        $path = Join-Path $fixtureWorkspace 'crates/fixture.rs'
        $original = [IO.File]::ReadAllBytes($path)
        try {
            [IO.File]::AppendAllText($path, 'changed')
            $rejected = $false
            try { $null = Get-PanzoCandidate -BuildDirectory $fixtureBuild -Workspace $fixtureWorkspace } catch { $rejected = $true }
            Assert $rejected 'Changed source accepted beside old binaries'
        } finally { [IO.File]::WriteAllBytes($path,$original) }
    }
    Check 'relative-path-after-location-change' {
        Push-Location $runRoot
        try {
            $relative = "output $unicode space"
            $resolved = Resolve-PanzoPath $relative
            Push-Location $workspace
            try {
                $null = New-Item -ItemType Directory -Path $resolved
                Assert ($resolved -eq (Join-Path $runRoot $relative)) 'Relative path did not bind to the caller directory'
                Assert (Test-Path -LiteralPath (Join-Path $runRoot $relative)) 'Output was written under the later working directory'
            } finally { Pop-Location }
        } finally { Pop-Location }
    }
    foreach ($script in @('verify-recording-editing.ps1','verify-re-mvp-final.ps1')) {
        Check "early-failure-report-$script" {
            $reportDir = Join-Path $runRoot "$script-report"
            $outputDir = Join-Path $runRoot "$script-copies"
            $arguments = @('-Executable',(Join-Path $runRoot 'missing/panzo-cli.exe'),'-ReportDirectory',$reportDir,'-OutputDirectory',$outputDir)
            if ($script -eq 'verify-recording-editing.ps1') { $arguments += @('-ShortProject','missing-a','-LongProject','missing-b'); $reportName='automated-regression.json' }
            else { $arguments += @('-FixtureRoot','missing-fixtures'); $reportName='final-regression.json' }
            $result = Invoke-PanzoScriptCase -Name "early-$script" -ScriptPath (Join-Path $PSScriptRoot $script) -Arguments $arguments -SuccessPattern 'evidence:|Final regression:' -ReportDirectory $runRoot -WorkingDirectory $runRoot -TimeoutSeconds 30
            Assert ($result.status -eq 'Fail' -and $result.exitCode -ne 0) 'Actual regression script swallowed its setup failure'
            $saved = Get-Content -LiteralPath (Join-Path $reportDir $reportName) -Raw -Encoding UTF8 | ConvertFrom-Json
            Assert ($saved.status -eq 'Fail') 'Actual regression script did not write a failed report'
            Assert (@($saved.results | Where-Object status -eq 'NotRun').Count -eq $saved.requiredCases.Count) 'Setup failure lost required NotRun cases'
        }
    }
    Check 'package-has-no-legacy-build-default' {
        $result = Invoke-PanzoScriptCase -Name 'package-no-build' -ScriptPath "$PSScriptRoot/package-re-mvp.ps1" -Arguments @('-AcceptanceReport',(Join-Path $runRoot 'success.json')) -SuccessPattern 'Archive:' -ReportDirectory $runRoot -WorkingDirectory $runRoot -TimeoutSeconds 30
        Assert ($result.status -eq 'Fail' -and $result.exitCode -ne 0) 'Packaging accepted no explicit build directory'
    }
} finally {
    $failed = @($checks | Where-Object status -ne 'Pass')
    [ordered]@{status=$(if($failed.Count){'Fail'}else{'Pass'});scope='Validation fixtures only; no recording/GPU acceptance';results=$checks.ToArray()} | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $runRoot 'validation-regression.json') -Encoding UTF8
    Write-Host "Validation fixture evidence: $runRoot"
}
if (@($checks | Where-Object status -ne 'Pass').Count) { throw 'Validation fixture regression failed' }
Write-Host "Validation fixture regression passed: $($checks.Count)/$($checks.Count)"
