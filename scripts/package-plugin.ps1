param(
 [Parameter(Mandatory=$true)][string]$Executable,
 [Parameter(Mandatory=$true)][string]$Manifest,
 [Parameter(Mandatory=$true)][string]$OutputPath
)
$ErrorActionPreference='Stop'
$exePath=(Resolve-Path -LiteralPath $Executable).Path
$manifestPath=(Resolve-Path -LiteralPath $Manifest).Path
$metadata=Get-Content -LiteralPath $manifestPath -Raw -Encoding UTF8 | ConvertFrom-Json
if ($metadata.protocol -notin @(1,2)) { throw 'Only plugin protocols 1 and 2 are supported' }
$bytes=[IO.File]::ReadAllBytes($exePath)
if ($bytes.Length -gt 32MB -or $bytes.Length -lt 64) { throw 'Invalid executable size' }
$metadata.executable_sha256=(Get-FileHash -LiteralPath $exePath -Algorithm SHA256).Hash.ToLowerInvariant()
$package=[ordered]@{format=1;manifest=$metadata;executable_base64=[Convert]::ToBase64String($bytes)}
$output=[IO.Path]::GetFullPath($OutputPath)
$null=New-Item -ItemType Directory -Force -Path ([IO.Path]::GetDirectoryName($output))
$json=$package | ConvertTo-Json -Depth 12 -Compress
[IO.File]::WriteAllText($output,$json,[Text.UTF8Encoding]::new($false))
[ordered]@{file=[IO.Path]::GetFileName($output);pluginId=$metadata.id;version=$metadata.version;protocol=$metadata.protocol;target=$metadata.target;sha256=(Get-FileHash -LiteralPath $output -Algorithm SHA256).Hash.ToLowerInvariant();bytes=(Get-Item -LiteralPath $output).Length} | ConvertTo-Json | Set-Content -LiteralPath ($output+'.sha256.json') -Encoding UTF8
Write-Host "Plugin package: $output"
