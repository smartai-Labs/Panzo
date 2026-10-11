param(
    [ValidateSet('zh-CN','zh-TW','en-US')][string]$Language = 'zh-CN',
    [switch]$Install,
    [string]$OfflineSource
)
# No installation or elevation on the default read-only path.
$ErrorActionPreference = 'Stop'
$names = @("Language.Basic~~~$Language~0.0.1.0", "Language.OCR~~~$Language~0.0.1.0")
if (-not $Install) {
    Add-Type -AssemblyName System.Runtime.WindowsRuntime
    $null = [Windows.Media.Ocr.OcrEngine,Windows.Foundation,ContentType=WindowsRuntime]
    [Windows.Media.Ocr.OcrEngine]::AvailableRecognizerLanguages | Select-Object LanguageTag,NativeName
    return
}
$principal = [Security.Principal.WindowsPrincipal]::new([Security.Principal.WindowsIdentity]::GetCurrent())
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { throw 'Installing Windows OCR needs an elevated PowerShell. Panzo also provides OCR language management in its image tools menu.' }
if ($OfflineSource) {
    $source = (Resolve-Path -LiteralPath $OfflineSource).ProviderPath
    if (-not (Test-Path -LiteralPath $source -PathType Container)) { throw 'OfflineSource must contain Windows language capability packages matching the OS version' }
}
foreach ($name in $names) {
    $current = Get-WindowsCapability -Online -Name $name
    if ($current.State -eq 'Installed') { Write-Output "$name already installed"; continue }
    $parameters = @{ Online=$true; Name=$name; ErrorAction='Stop' }
    if ($OfflineSource) { $parameters.Source=$source; $parameters.LimitAccess=$true }
    Add-WindowsCapability @parameters
}
