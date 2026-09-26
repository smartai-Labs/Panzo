param(
    [Parameter(Mandatory=$true)][string]$TargetDirectory
)
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot/validation-common.ps1"
$workspace = Resolve-PanzoPath (Split-Path -Parent $PSScriptRoot)
$target = Resolve-PanzoPath $TargetDirectory
$build = Join-Path $target 'release'
$null = New-Item -ItemType Directory -Force -Path $build
$manifestPath = Join-Path $build 'candidate-build.json'
# A failed rebuild must not leave a previously valid identity beside new binaries.
if (Test-Path -LiteralPath $manifestPath) { Remove-Item -LiteralPath $manifestPath -Force }
$source = Get-PanzoSourceIdentity $workspace
$arguments = @('build','--locked','--offline','-p','panzo-app','--release','--bins','--target-dir',$target)
$result = Invoke-PanzoProcess -Executable (Get-Command cargo -CommandType Application).Source -Arguments $arguments -WorkingDirectory $workspace -StdoutPath (Join-Path $build 'candidate-build.stdout.txt') -StderrPath (Join-Path $build 'candidate-build.stderr.txt') -TimeoutSeconds 1800
if ($result.watchdogExpired -or $result.exitCode -ne 0) { throw "Candidate build failed; see $build\candidate-build.stderr.txt" }
if ((Get-PanzoSourceIdentity $workspace).sha256 -ne $source.sha256) { throw 'Source/configuration changed during build; rebuild when edits are complete' }
[ordered]@{
    schemaVersion=1;configuration='release';builtAtUtc=[DateTime]::UtcNow.ToString('o')
    arguments=$arguments;source=$source;binaries=@(Get-PanzoBinaryIdentity $build)
} | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $manifestPath -Encoding UTF8
Write-Host "Candidate build: $build"
Write-Host "Identity: $manifestPath"
