@echo off
if not exist "%~dp0target\release\panzo.exe" (
    echo Build Panzo first from the repository root:
    echo cargo build --workspace --release --locked
    pause
    exit /b 1
)
start "" "%~dp0target\release\panzo.exe"
