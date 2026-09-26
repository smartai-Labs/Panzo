# Internal validation helpers. Dot-source this file; it does not run a probe.

function Resolve-PanzoPath {
    param([Parameter(Mandatory=$true)][string]$Path, [string]$BaseDirectory = (Get-Location).ProviderPath)
    if ([IO.Path]::IsPathRooted($Path)) { return [IO.Path]::GetFullPath($Path) }
    return [IO.Path]::GetFullPath((Join-Path $BaseDirectory $Path))
}

function ConvertTo-PanzoProcessArgument {
    param([AllowEmptyString()][string]$Value)
    # Windows CommandLineToArgvW / CRT quoting, including a trailing backslash.
    return '"' + ([regex]::Replace(([regex]::Replace($Value, '(\\*)"', '$1$1\"')), '(\\+)$', '$1$1')) + '"'
}

function New-PanzoProcessJob {
    # A Job owns only this validation process and its descendants. Closing it
    # stops an entire timed-out script/probe tree without enumerating user processes.
    if (-not ('PanzoValidationProcessJob' -as [type])) {
        Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Diagnostics;
using System.Runtime.InteropServices;
public sealed class PanzoValidationProcessJob : IDisposable {
    [StructLayout(LayoutKind.Sequential)] struct Basic {
        public long ProcessTime, JobTime; public uint Flags;
        public UIntPtr MinWorking, MaxWorking; public uint ActiveProcesses;
        public UIntPtr Affinity; public uint Priority, Scheduling;
    }
    [StructLayout(LayoutKind.Sequential)] struct Io {
        public ulong ReadOps, WriteOps, OtherOps, ReadBytes, WriteBytes, OtherBytes;
    }
    [StructLayout(LayoutKind.Sequential)] struct Extended {
        public Basic Basic; public Io Io;
        public UIntPtr ProcessMemory, JobMemory, PeakProcessMemory, PeakJobMemory;
    }
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    static extern IntPtr CreateJobObject(IntPtr attributes, string name);
    [DllImport("kernel32.dll", SetLastError=true)]
    static extern bool SetInformationJobObject(IntPtr job, int infoClass, ref Extended info, uint size);
    [DllImport("kernel32.dll", SetLastError=true)]
    static extern bool AssignProcessToJobObject(IntPtr job, IntPtr process);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr handle);
    IntPtr handle;
    public PanzoValidationProcessJob() {
        handle=CreateJobObject(IntPtr.Zero,null);
        if(handle==IntPtr.Zero) throw new Win32Exception();
        var limits=new Extended(); limits.Basic.Flags=0x2000;
        if(!SetInformationJobObject(handle,9,ref limits,(uint)Marshal.SizeOf(typeof(Extended)))) {
            var error=new Win32Exception(); Dispose(); throw error;
        }
    }
    public void Attach(Process process) {
        if(!AssignProcessToJobObject(handle,process.Handle)) throw new Win32Exception();
    }
    public void Dispose() { if(handle!=IntPtr.Zero) { CloseHandle(handle); handle=IntPtr.Zero; } }
}
'@
    }
    return [PanzoValidationProcessJob]::new()
}

function Invoke-PanzoProcess {
    param(
        [Parameter(Mandatory=$true)][string]$Executable,
        [string[]]$Arguments = @(),
        [Parameter(Mandatory=$true)][string]$WorkingDirectory,
        [Parameter(Mandatory=$true)][string]$StdoutPath,
        [Parameter(Mandatory=$true)][string]$StderrPath,
        [ValidateRange(1,86400)][int]$TimeoutSeconds = 120
    )
    $job = New-PanzoProcessJob
    $process = [Diagnostics.Process]::new()
    $process.StartInfo.FileName = $Executable
    $process.StartInfo.Arguments = ($Arguments | ForEach-Object { ConvertTo-PanzoProcessArgument $_ }) -join ' '
    $process.StartInfo.WorkingDirectory = $WorkingDirectory
    $process.StartInfo.UseShellExecute = $false
    $process.StartInfo.CreateNoWindow = $true
    $process.StartInfo.RedirectStandardOutput = $true
    $process.StartInfo.RedirectStandardError = $true
    $process.StartInfo.StandardOutputEncoding = [Text.UTF8Encoding]::new($false)
    $process.StartInfo.StandardErrorEncoding = [Text.UTF8Encoding]::new($false)
    $watch = [Diagnostics.Stopwatch]::StartNew()
    $started = $false; $expired = $false; $peakWorking = 0L; $peakPrivate = 0L
    $outputTask = $null; $errorTask = $null
    try {
        $started = $process.Start()
        $job.Attach($process)
        $outputTask = $process.StandardOutput.ReadToEndAsync()
        $errorTask = $process.StandardError.ReadToEndAsync()
        while (-not $process.WaitForExit(100)) {
            try {
                $process.Refresh()
                $peakWorking = [Math]::Max($peakWorking, $process.WorkingSet64)
                $peakPrivate = [Math]::Max($peakPrivate, $process.PrivateMemorySize64)
            } catch { if (-not $process.HasExited) { throw } }
            if ($watch.Elapsed.TotalSeconds -ge $TimeoutSeconds) {
                $expired = $true
                $job.Dispose()
                if (-not $process.WaitForExit(5000)) { throw 'Owned validation process did not stop after its timeout' }
                break
            }
        }
        if (-not $outputTask.Wait(5000) -or -not $errorTask.Wait(5000)) {
            throw 'Validation output streams did not close'
        }
        return [pscustomobject]@{
            exitCode=$process.ExitCode; watchdogExpired=$expired; elapsedMs=$watch.ElapsedMilliseconds
            peakWorkingSetBytes=$peakWorking; peakPrivateBytes=$peakPrivate
            output=$outputTask.GetAwaiter().GetResult(); errors=$errorTask.GetAwaiter().GetResult()
        }
    } finally {
        $job.Dispose()
        if ($started -and -not $process.HasExited) { $process.Kill(); $null=$process.WaitForExit(5000) }
        $output = if ($null -ne $outputTask -and $outputTask.IsCompleted) { $outputTask.GetAwaiter().GetResult() } else { '' }
        $errors = if ($null -ne $errorTask -and $errorTask.IsCompleted) { $errorTask.GetAwaiter().GetResult() } else { 'Process launch or output collection failed; see the parent report.' }
        [IO.File]::WriteAllText($StdoutPath, [string]$output, [Text.UTF8Encoding]::new($false))
        [IO.File]::WriteAllText($StderrPath, [string]$errors, [Text.UTF8Encoding]::new($false))
        $process.Dispose()
    }
}

function Get-PanzoSourceIdentity {
    param([Parameter(Mandatory=$true)][string]$Workspace)
    $paths = @(Get-ChildItem -LiteralPath (Join-Path $Workspace 'crates') -Recurse -File | ForEach-Object { $_.FullName })
    foreach ($name in @('Cargo.toml','Cargo.lock','rust-toolchain.toml','rustfmt.toml')) { $paths += Join-Path $Workspace $name }
    $lines = foreach ($path in ($paths | Sort-Object)) {
        $relative = $path.Substring($Workspace.TrimEnd('\','/').Length + 1).Replace('\','/')
        '{0}  {1}' -f (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash,$relative
    }
    $sha = [Security.Cryptography.SHA256]::Create()
    try {
        $hash = [BitConverter]::ToString($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes(($lines -join "`n")))).Replace('-','')
        return [pscustomobject]@{sha256=$hash;files=@($lines)}
    } finally { $sha.Dispose() }
}

function Get-PanzoBinaryIdentity {
    param([Parameter(Mandatory=$true)][string]$BuildDirectory)
    foreach ($name in @('panzo.exe','panzo-cli.exe')) {
        $path = Join-Path $BuildDirectory $name
        $file = Get-Item -LiteralPath $path -ErrorAction Stop
        if ($file.PSIsContainer) { throw "Not a binary file: $path" }
        [pscustomobject]@{name=$name;sha256=(Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash;bytes=$file.Length}
    }
}

function Assert-PanzoBinaryIdentity {
    param([Parameter(Mandatory=$true)]$Expected, [Parameter(Mandatory=$true)]$Actual)
    foreach ($name in @('panzo.exe','panzo-cli.exe')) {
        $before = @($Expected | Where-Object { $_.name -eq $name })
        $after = @($Actual | Where-Object { $_.name -eq $name })
        if ($before.Count -ne 1 -or $after.Count -ne 1 -or
            $before[0].sha256 -notmatch '^[A-Fa-f0-9]{64}$' -or
            $before[0].sha256 -ne $after[0].sha256 -or $before[0].bytes -ne $after[0].bytes) {
            throw "Candidate binary identity mismatch: $name"
        }
    }
}

function Get-PanzoCandidate {
    param([Parameter(Mandatory=$true)][string]$BuildDirectory, [Parameter(Mandatory=$true)][string]$Workspace)
    $build = Resolve-PanzoPath $BuildDirectory
    $manifestPath = Join-Path $build 'candidate-build.json'
    if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) {
        throw "Missing candidate-build.json. Build this candidate with scripts/build-candidate.ps1: $build"
    }
    $manifest = Get-Content -LiteralPath $manifestPath -Raw -Encoding UTF8 | ConvertFrom-Json
    if ($manifest.schemaVersion -ne 1 -or $manifest.configuration -ne 'release') { throw 'Unsupported candidate build manifest' }
    $identity = @(Get-PanzoBinaryIdentity $build)
    Assert-PanzoBinaryIdentity $manifest.binaries $identity
    $source = Get-PanzoSourceIdentity $Workspace
    if ($manifest.source.sha256 -ne $source.sha256) { throw 'Candidate was built from different source/configuration; rebuild before validation or packaging' }
    return [pscustomobject]@{directory=$build;sourceSha256=$source.sha256;binaries=$identity;manifestPath=$manifestPath}
}

function Get-PanzoRequiredUnifiedCases {
    param([switch]$Extended)
    foreach ($fixture in @('F01','F02')) {
        "$fixture-end-seek"
        foreach ($round in 1..3) { "$fixture-scrub-$round"; "$fixture-queued-play-$round" }
        "$fixture-paused"
    }
    'RE-LIFE-01-20-cycles'; 'RE-CUT-EXPORT-RENDER'; 'RE-SAVE-01-ten-faults'
    'F06-window-recording'; 'F06-CUT-EXPORT-RENDER'; 'RE-REC-native-controller'
    if ($Extended) {
        'F03-60-second-recording'; 'F03-60-second-playback'; 'F03-proxy-preparation'
        foreach ($round in 1..3) { "F03-scrub-$round" }
        'RE-STABLE-01-30-minutes'; 'RE-STABLE-01-five-recoveries'
    }
}

function Get-PanzoRequiredFinalCases {
    param([switch]$PreparedLongProject)
    'short-unified'; 'F03-cold-60s-play'; 'F03-first-proxy-preparation'; 'F03-warm-60s-play'
    foreach ($round in 1..3) { "F03-scrub-$round" }
    if ($PreparedLongProject) { 'F05-cached-end-seek'; 'F05-existing-proxy-open' }
    else { 'F05-cold-end-seek'; 'F05-first-proxy-preparation' }
    foreach ($round in 1..3) { "F05-scrub-$round" }
    'F05-five-minute-scrub-resources'; 'fmt'; 'clippy'; 'unit-tests'
}

function Get-PanzoCaseFailures {
    param([AllowEmptyCollection()][object[]]$Results, [string[]]$RequiredCases)
    if (@($RequiredCases).Count -eq 0) { 'Required case list is empty' }
    foreach ($name in $RequiredCases) {
        $matchingCases = @($Results | Where-Object { $_.caseId -eq $name })
        if ($matchingCases.Count -ne 1) { "Missing or duplicate required case: $name" }
        elseif ($matchingCases[0].status -ne 'Pass') { "Required case did not pass: $name ($($matchingCases[0].status))" }
    }
    foreach ($result in $Results) {
        if ($result.status -notin @('Pass','NotRun','Deferred')) { "Case failed: $($result.caseId) ($($result.status))" }
    }
}

function Complete-PanzoRegressionReport {
    param(
        [Parameter(Mandatory=$true)][string]$Path,
        [Parameter(Mandatory=$true)][Collections.IDictionary]$Report,
        [Parameter(Mandatory=$true)][string[]]$RequiredCases,
        $Candidate, [Parameter(Mandatory=$true)][string]$Workspace,
        [string]$RunError = ''
    )
    $failures = @(Get-PanzoCaseFailures -Results @($Report.results) -RequiredCases $RequiredCases)
    if ($RunError) { $failures += $RunError }
    $unchanged = $false
    try {
        if ($null -eq $Candidate) { throw 'Candidate identity was not established' }
        Assert-PanzoBinaryIdentity $Candidate.binaries @(Get-PanzoBinaryIdentity $Candidate.directory)
        if ((Get-PanzoSourceIdentity $Workspace).sha256 -ne $Candidate.sourceSha256) { throw 'Source/configuration changed during validation' }
        $unchanged = $true
    } catch { $failures += $_.Exception.Message }
    $allResults = @($Report.results)
    foreach ($name in $RequiredCases) {
        if (-not @($allResults | Where-Object { $_.caseId -eq $name }).Count) {
            $allResults += [ordered]@{caseId=$name;status='NotRun';error='Run stopped before this required case completed'}
        }
    }
    $Report.schemaVersion=2; $Report.requiredCases=$RequiredCases; $Report.results=$allResults
    $Report.build=$Candidate; $Report.buildUnchanged=$unchanged; $Report.failures=$failures
    $Report.status=$(if ($failures.Count -eq 0) { 'Pass' } else { 'Fail' })
    $Report.capturedAtUtc=[DateTime]::UtcNow.ToString('o')
    $Report | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath $Path -Encoding UTF8
    return [pscustomobject]$Report
}

function Assert-PanzoRegressionReport {
    param([Parameter(Mandatory=$true)][string]$Path, $Candidate, [string[]]$RequiredCases = @(), [int]$Depth = 0)
    if ($Depth -gt 3) { throw 'Nested regression report depth exceeded' }
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw "Missing regression report: $Path" }
    $report = Get-Content -LiteralPath $Path -Raw -Encoding UTF8 | ConvertFrom-Json
    if ($report.schemaVersion -ne 2 -or $report.status -ne 'Pass' -or $report.buildUnchanged -ne $true -or @($report.failures).Count -ne 0) {
        throw "Regression report did not pass completely: $Path"
    }
    $contract = @()
    if ($report.reportType -eq 'unified-regression') { $contract = @(Get-PanzoRequiredUnifiedCases -Extended:([bool]$report.extended)) }
    if ($report.reportType -eq 'final-regression') { $contract = @(Get-PanzoRequiredFinalCases -PreparedLongProject:([bool]$report.preparedLongProject)) }
    $required = @($report.requiredCases) + @($RequiredCases) + $contract | Select-Object -Unique
    $failures = @(Get-PanzoCaseFailures -Results @($report.results) -RequiredCases $required)
    if ($failures.Count) { throw ($failures -join '; ') }
    if ($null -ne $Candidate) {
        if ($report.build.sourceSha256 -ne $Candidate.sourceSha256) { throw 'Regression source identity differs from candidate' }
        Assert-PanzoBinaryIdentity $Candidate.binaries $report.build.binaries
    }
    if ($report.reportType -eq 'final-regression') {
        if (@($report.nestedReports).Count -ne 1 -or $report.nestedReports[0] -ne 'short-unified/automated-regression.json') {
            throw 'Final report is missing the declared unified regression evidence'
        }
        $nested = Join-Path (Split-Path -Parent (Resolve-PanzoPath $Path)) 'short-unified/automated-regression.json'
        $null = Assert-PanzoRegressionReport -Path $nested -Candidate $Candidate -RequiredCases @(Get-PanzoRequiredUnifiedCases) -Depth ($Depth + 1)
    }
    return $report
}

function Invoke-PanzoScriptCase {
    param(
        [Parameter(Mandatory=$true)][string]$Name, [Parameter(Mandatory=$true)][string]$ScriptPath,
        [string[]]$Arguments = @(), [Parameter(Mandatory=$true)][string]$SuccessPattern,
        [Parameter(Mandatory=$true)][string]$ReportDirectory, [Parameter(Mandatory=$true)][string]$WorkingDirectory,
        [int]$TimeoutSeconds = 300, [string]$ExpectedReport = '', $Candidate,
        [string[]]$RequiredCases = @()
    )
    $out = Join-Path $ReportDirectory "$Name.stdout.txt"; $err = Join-Path $ReportDirectory "$Name.stderr.txt"
    $result = [ordered]@{caseId=$Name;status='Fail';stdout=$out;stderr=$err;exitCode=$null;error=$null}
    try {
        # -File owns the child script's status; a deliberately failed native probe
        # does not leak its LASTEXITCODE into an otherwise successful script.
        $shell = Join-Path $PSHOME 'powershell.exe'
        if (-not (Test-Path -LiteralPath $shell)) { $shell = Join-Path $PSHOME 'pwsh.exe' }
        $process = Invoke-PanzoProcess -Executable $shell -Arguments (@('-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',$ScriptPath) + $Arguments) -WorkingDirectory $WorkingDirectory -StdoutPath $out -StderrPath $err -TimeoutSeconds $TimeoutSeconds
        $result.exitCode=$process.exitCode; $result.elapsedMs=$process.elapsedMs; $result.watchdogExpired=$process.watchdogExpired
        if ($process.watchdogExpired -or $process.exitCode -ne 0) { throw "Child script failed with exit $($process.exitCode); timeout=$($process.watchdogExpired)" }
        if ($process.output -notmatch $SuccessPattern) { throw 'Child script did not provide its required completion evidence' }
        if ($ExpectedReport) { $null = Assert-PanzoRegressionReport -Path $ExpectedReport -Candidate $Candidate -RequiredCases $RequiredCases }
        $result.status='Pass'
    } catch { $result.error=$_.Exception.Message }
    return [pscustomobject]$result
}
