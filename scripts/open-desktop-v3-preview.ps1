# Opens the local design preview in its own visible app window.
# This does not replace or launch the native Panzo executable.
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$previewFile = Join-Path $projectRoot 'docs\prototypes\desktop-v3\index.html'
$previewProfile = Join-Path $projectRoot '.tmp\desktop-v3-preview-profile'
$browserPath = @(
    'C:\Program Files\Google\Chrome\Application\chrome.exe',
    'C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe'
) | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
if (-not $browserPath) { throw 'Chrome or Edge is required for the design preview.' }
if (-not (Test-Path -LiteralPath $previewFile)) { throw 'Desktop v3 preview is missing.' }
New-Item -ItemType Directory -Path $previewProfile -Force | Out-Null
$previewUri = ([System.Uri]$previewFile).AbsoluteUri
$browserArgs = @(
    ('--app="' + $previewUri + '"'),
    ('--user-data-dir="' + $previewProfile + '"'),
    '--window-size=1440,960',
    '--no-first-run',
    '--no-default-browser-check',
    '--disable-extensions'
)
# Intentional visible window: this is the interactive preview the user is reviewing.
Start-Process -FilePath $browserPath -ArgumentList $browserArgs
Write-Output ('Opened desktop design preview: ' + $previewFile)

