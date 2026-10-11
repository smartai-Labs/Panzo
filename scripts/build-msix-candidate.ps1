param(
    [Parameter(Mandatory=$true)][string]$BuildDirectory,
    [string]$OutputDirectory = 'dist',
    [string]$Publisher = 'CN=Panzo',
    [ValidatePattern('^\d+\.\d+\.\d+\.\d+$')][string]$Version = '0.4.0.0',
    [string]$CertificateThumbprint
)
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot/validation-common.ps1"
$workspace = Resolve-PanzoPath (Split-Path -Parent $PSScriptRoot)
$candidate = Get-PanzoCandidate -BuildDirectory (Resolve-PanzoPath $BuildDirectory) -Workspace $workspace
$destination = Resolve-PanzoPath $OutputDirectory
$null = New-Item -ItemType Directory -Force -Path $destination
$staging = Join-Path $destination ('Panzo-msix-' + [Guid]::NewGuid().ToString('N'))
$null = New-Item -ItemType Directory -Path $staging
$assets = Join-Path $staging 'Assets'
$null = New-Item -ItemType Directory -Path $assets
$kitRoot = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits/10/bin'
$makeappx = Get-ChildItem -LiteralPath $kitRoot -Directory | Sort-Object Name -Descending | ForEach-Object { Join-Path $_.FullName 'x64/makeappx.exe' } | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
if (-not $makeappx) { throw 'Windows SDK MakeAppx was not found' }
foreach ($name in @('panzo.exe','panzo-cli.exe','candidate-build.json')) { Copy-Item -LiteralPath (Join-Path (Resolve-PanzoPath $BuildDirectory) $name) -Destination (Join-Path $staging $name) }
Copy-Item -LiteralPath (Join-Path $workspace 'LICENSE') -Destination $staging
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'manage-ocr-capability.ps1') -Destination $staging
# Reuse the same vector-derived branding as the EXE, windows and tray.
foreach ($size in @(44,50,150)) {
    Copy-Item -LiteralPath (Join-Path $workspace "crates/panzo-windows/assets/panzo-$size.png") -Destination (Join-Path $assets "Logo$size.png")
}
$escapedPublisher = [Security.SecurityElement]::Escape($Publisher)
@"
<?xml version="1.0" encoding="utf-8"?>
<Package xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10" xmlns:uap="http://schemas.microsoft.com/appx/manifest/uap/windows10" xmlns:rescap="http://schemas.microsoft.com/appx/manifest/foundation/windows10/restrictedcapabilities" IgnorableNamespaces="uap rescap">
  <Identity Name="Panzo.Desktop" Publisher="$escapedPublisher" Version="$Version" ProcessorArchitecture="x64" />
  <Properties><DisplayName>Panzo</DisplayName><PublisherDisplayName>Panzo</PublisherDisplayName><Logo>Assets\Logo50.png</Logo></Properties>
  <Resources><Resource Language="zh-cn" /><Resource Language="en-us" /></Resources>
  <Dependencies><TargetDeviceFamily Name="Windows.Desktop" MinVersion="10.0.19041.0" MaxVersionTested="10.0.26100.0" /></Dependencies>
  <Applications><Application Id="Panzo" Executable="panzo.exe" EntryPoint="Windows.FullTrustApplication"><uap:VisualElements DisplayName="Panzo" Description="Screenshots, desktop pins and screen recording" Square150x150Logo="Assets\Logo150.png" Square44x44Logo="Assets\Logo44.png" BackgroundColor="transparent" /></Application></Applications>
  <Capabilities><rescap:Capability Name="runFullTrust" /></Capabilities>
</Package>
"@ | Set-Content -LiteralPath (Join-Path $staging 'AppxManifest.xml') -Encoding UTF8
# Include the license notices of the dependencies in this exact offline build.
$metadata = Invoke-PanzoProcess -Executable (Get-Command cargo -CommandType Application).Source -Arguments @('metadata','--locked','--offline','--filter-platform','x86_64-pc-windows-msvc','--format-version','1') -WorkingDirectory $workspace -StdoutPath (Join-Path $destination 'msix-dependencies.json') -StderrPath (Join-Path $destination 'msix-dependencies.log')
if ($metadata.exitCode -ne 0 -or $metadata.watchdogExpired) { throw 'Dependency metadata failed' }
$licenses = Join-Path $staging 'third-party-licenses'
$null = New-Item -ItemType Directory -Path $licenses
foreach ($dependency in ($metadata.output | ConvertFrom-Json).packages | Where-Object { $null -ne $_.source }) {
    $folder = Join-Path $licenses "$($dependency.name)-$($dependency.version)"
    $null = New-Item -ItemType Directory -Path $folder
    foreach ($license in Get-ChildItem -LiteralPath (Split-Path -Parent $dependency.manifest_path) -File | Where-Object { $_.Name -match '^(LICENSE|COPYING|NOTICE)' }) { Copy-Item -LiteralPath $license.FullName -Destination $folder }
}
$package = Join-Path $destination "Panzo-$Version-x64.msix"
if (Test-Path -LiteralPath $package) { throw "Package already exists: $package" }
& $makeappx pack /d $staging /p $package /o
if ($LASTEXITCODE -ne 0) { throw 'MakeAppx validation/build failed' }
$signed = $false
if ($CertificateThumbprint) {
    $cert = Get-Item -LiteralPath "Cert:\CurrentUser\My\$CertificateThumbprint"
    if (-not $cert.HasPrivateKey -or $cert.Subject -ne $Publisher) { throw 'Signing certificate must have a private key and match Publisher exactly' }
    $signtool = Join-Path (Split-Path -Parent $makeappx) 'signtool.exe'
    & $signtool sign /sha1 $CertificateThumbprint /fd SHA256 $package
    if ($LASTEXITCODE -ne 0) { throw 'Signing failed' }
    $signed = $true
}
[ordered]@{schemaVersion=1;package=$package;sha256=(Get-FileHash -LiteralPath $package).Hash;sourceSha256=$candidate.sourceSha256;signed=$signed;installed=$false;makeappxValidated=$true;ocrCapability='Windows Language.OCR; managed inside Panzo or via included manage-ocr-capability.ps1';releaseAcceptance=$false} | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath "$package.json" -Encoding UTF8
Write-Host "Created: $package (signed: $signed). Package was not installed."
