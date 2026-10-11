param(
    [Parameter(Mandatory=$true)][string]$BuildDirectory,
    [Parameter(Mandatory=$true)][string]$AcceptanceReport,
    [string]$Label = 'candidate-' + (Get-Date -Format 'yyyyMMdd-HHmmss')
)
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot/validation-common.ps1"
$workspace = [IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot))
if ($Label -notmatch '^[a-zA-Z0-9-]+$') { throw 'Package label must be alphanumeric with hyphens' }
$build = Resolve-PanzoPath $BuildDirectory
$acceptedPath = Resolve-PanzoPath $AcceptanceReport
$candidate = Get-PanzoCandidate -BuildDirectory $build -Workspace $workspace
$accepted = Assert-PanzoRegressionReport -Path $acceptedPath -Candidate $candidate
if ($accepted.reportType -ne 'final-regression') { throw 'Packaging requires a complete final-regression.json, not a partial or historical report' }
$dist = Join-Path $workspace 'dist'
$package = Join-Path $dist "Panzo-RE-MVP-$Label"
if (Test-Path -LiteralPath $package) { throw "Package already exists: $package" }
$null = New-Item -ItemType Directory -Path $package
$files = @()
foreach ($name in @('panzo.exe','panzo-cli.exe')) {
    $source = Join-Path $build $name
    $destination = Join-Path $package $name
    Copy-Item -LiteralPath $source -Destination $destination
    $hash = (Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash
    if ($hash -ne (Get-FileHash -LiteralPath $source).Hash) { throw "Copy hash mismatch: $name" }
    $bytes = [IO.File]::ReadAllBytes($destination)
    $pe = [BitConverter]::ToInt32($bytes,0x3c)
    $subsystem = [BitConverter]::ToUInt16($bytes,$pe + 24 + 68)
    if ($name -eq 'panzo.exe' -and $subsystem -ne 2) { throw 'GUI candidate is not Windows subsystem' }
    $files += [ordered]@{name=$name;sha256=$hash;bytes=$bytes.Length;peSubsystem=$subsystem}
}
Copy-Item -LiteralPath (Join-Path $workspace 'README.md') -Destination (Join-Path $package 'SOURCE-README.md')
Copy-Item -LiteralPath (Join-Path $workspace 'LICENSE') -Destination (Join-Path $package 'LICENSE')
# The historical release description is not the identity of a new candidate.
# Generate a neutral entry point bound to the evidence checked above.
@"
# Panzo candidate $Label

Open panzo.exe. panzo-cli.exe is the diagnostic entry point.

This package passed the automated scope in [the exact-binary report](validation/final-regression.json).
It is not an overall release acceptance or a claim that manual visual checks passed.
Source/configuration SHA256: $($candidate.sourceSha256)

See [build and usage instructions](SOURCE-README.md).
Only evidence for this candidate is bundled; local design history is not required.
"@ | Set-Content -LiteralPath (Join-Path $package 'README.md') -Encoding UTF8
Copy-Item -LiteralPath $candidate.manifestPath -Destination (Join-Path $package 'candidate-build.json')
$validationRoot = Join-Path $package 'validation'
$null = New-Item -ItemType Directory -Path $validationRoot
$evidenceRoot = Split-Path -Parent $acceptedPath
foreach ($entry in Get-ChildItem -LiteralPath $evidenceRoot -Recurse -File | Where-Object { $_.Extension -in @('.json','.txt','.log') }) {
    $relative = $entry.FullName.Substring($evidenceRoot.Length + 1)
    $destination = Join-Path $validationRoot $relative
    $null = New-Item -ItemType Directory -Force -Path (Split-Path -Parent $destination)
    Copy-Item -LiteralPath $entry.FullName -Destination $destination
}
Copy-Item -LiteralPath $acceptedPath -Destination (Join-Path $validationRoot 'final-regression.json') -Force
$metadataProcess = Invoke-PanzoProcess -Executable (Get-Command cargo -CommandType Application).Source -Arguments @('metadata','--locked','--offline','--filter-platform','x86_64-pc-windows-msvc','--format-version','1','--manifest-path',(Join-Path $workspace 'Cargo.toml')) -WorkingDirectory $workspace -StdoutPath (Join-Path $package 'dependency-metadata.json') -StderrPath (Join-Path $package 'dependency-metadata.stderr.txt')
if ($metadataProcess.watchdogExpired -or $metadataProcess.exitCode -ne 0) { throw 'Dependency metadata failed' }
$metadata = $metadataProcess.output | ConvertFrom-Json
$licensesRoot = Join-Path $package 'third-party-licenses'
$null = New-Item -ItemType Directory -Path $licensesRoot
$dependencies = @()
foreach ($dependency in $metadata.packages | Where-Object { $null -ne $_.source }) {
    $sourceRoot = Split-Path -Parent $dependency.manifest_path
    $licenseFiles = @(Get-ChildItem -LiteralPath $sourceRoot -File | Where-Object { $_.Name -match '^(LICENSE|COPYING|NOTICE)' })
    if ($licenseFiles.Count -gt 0) {
        $destination = Join-Path $licensesRoot "$($dependency.name)-$($dependency.version)"
        $null = New-Item -ItemType Directory -Path $destination
        foreach ($license in $licenseFiles) { Copy-Item -LiteralPath $license.FullName -Destination (Join-Path $destination $license.Name) }
    }
    $dependencies += [ordered]@{name=$dependency.name;version=$dependency.version;license=$dependency.license;repository=$dependency.repository;licenseFiles=@($licenseFiles.Name)}
}
$dependencies | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $package 'dependencies.json') -Encoding UTF8
[ordered]@{
    label=$Label;builtFrom=$build;packagedAtUtc=[DateTime]::UtcNow.ToString('o');
    releaseAccepted=$false;scope='Local candidate; see acceptance report. No cross-GPU, audio, or visual acceptance claim.';
    files=$files;sourceSha256=$candidate.sourceSha256;automatedReport='validation/final-regression.json';automatedStatus=$accepted.status
} | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $package 'build-manifest.json') -Encoding UTF8
$currentCandidate = Get-PanzoCandidate -BuildDirectory $build -Workspace $workspace
Assert-PanzoBinaryIdentity $candidate.binaries $currentCandidate.binaries
Assert-PanzoBinaryIdentity $candidate.binaries @(Get-PanzoBinaryIdentity $package)
$null = Assert-PanzoRegressionReport -Path (Join-Path $validationRoot 'final-regression.json') -Candidate $candidate
$archive = "$package.zip"
# Some Cargo archives use a pre-1980 timestamp. ZIP cannot encode it. Normalize
# only packaged copies, never source crates or the user's project files.
foreach ($entry in Get-ChildItem -LiteralPath $package -File -Recurse) {
    if ($entry.LastWriteTime.Year -lt 1980 -or $entry.LastWriteTime.Year -gt 2107) {
        $entry.LastWriteTime = [DateTime]::new(2000,1,1,0,0,0)
    }
}
Compress-Archive -LiteralPath $package -DestinationPath $archive
Write-Host "Candidate: $package"
Write-Host "Archive: $archive"
Get-FileHash -LiteralPath $archive -Algorithm SHA256
