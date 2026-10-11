# Open an interactive design preview; this is not the native Panzo executable.
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$previewFile = Join-Path $projectRoot 'docs\prototypes\workbench-v14\index.html'
$previewProfile = Join-Path $projectRoot '.tmp\workbench-v14-preview-profile'
$browserPath = @(
    'C:\Program Files\Google\Chrome\Application\chrome.exe',
    'C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe',
    'C:\Program Files\Microsoft\Edge\Application\msedge.exe'
) | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
if (-not $browserPath) { throw 'Chrome or Edge is required for this design preview.' }
if (-not (Test-Path -LiteralPath $previewFile)) { throw 'Workbench v14 design preview is missing.' }
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
# Intentional visible window: the user is reviewing this interactive prototype.
Start-Process -FilePath $browserPath -ArgumentList $browserArgs
Write-Output ('Opened workbench v14 layout prototype: ' + $previewFile)
