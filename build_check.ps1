# Build verification script
Write-Host "=== Checking exness-api ===" -ForegroundColor Cyan
cargo build -p exness-api 2>&1 | Select-String "(error\[|warning:)" | Select-Object -First 20

Write-Host "`n=== Checking gl-strategy ===" -ForegroundColor Cyan
cargo build -p gl-strategy 2>&1 | Select-String "(error\[|warning:)" | Select-Object -First 20

Write-Host "`n=== Checking gl-runtime ===" -ForegroundColor Cyan
cargo build -p gl-runtime 2>&1 | Select-String "(error\[|warning:)" | Select-Object -First 20

Write-Host "`n=== Build Summary ===" -ForegroundColor Green
cargo build --workspace 2>&1 | Select-String "(Finished|error:)" | Select-Object -First 5
