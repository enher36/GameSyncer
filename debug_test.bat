@echo off
echo Starting GameSyncer with debug output...
echo.
echo This will show detailed debug logs to help identify upload issues.
echo Press Ctrl+C to stop.
echo.
timeout /t 3
cargo run --bin steam-cloud-sync 2>&1