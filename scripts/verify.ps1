param(
    [switch]$IncludeMediaProbe,
    [switch]$IncludeInputProbe
)

$ErrorActionPreference = 'Stop'
$workspace = Split-Path -Parent $PSScriptRoot
Push-Location $workspace

try {
    cargo fmt --all -- --check
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    cargo test --workspace --all-targets
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    cargo clippy --workspace --all-targets -- -D warnings
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    cargo run -p panzo-app --bin panzo-cli
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    cargo run -p panzo-app --bin panzo-cli -- encoder-probe
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    cargo run -p panzo-app --bin panzo-cli -- video-probe
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    if ($IncludeMediaProbe) {
        $temporaryDirectory = Join-Path $workspace '.tmp'
        New-Item -ItemType Directory -Force $temporaryDirectory | Out-Null
        $probeOutput = Join-Path $temporaryDirectory 'verify-fmp4.part.mp4'
        cargo run -p panzo-app --bin panzo-cli -- fmp4-probe $probeOutput 2
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    }

    if ($IncludeInputProbe) {
        cargo run -p panzo-app --bin panzo-cli -- input-probe 1
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    }
}
finally {
    Pop-Location
}
