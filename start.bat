@echo off
REM ============================================================
REM  GL Server - Windows 启动脚本
REM  行为：先停止所有 gl-server 进程，再启动服务
REM ============================================================
setlocal enabledelayedexpansion

REM 定位到脚本所在目录（项目根目录）
cd /d "%~dp0"

echo [1/3] Stopping existing gl-server processes...
taskkill /F /IM gl-server.exe >nul 2>&1
if errorlevel 1 (
    echo       No running gl-server.exe process found.
) else (
    echo       Existing gl-server.exe terminated.
)

REM 等待端口释放（监听 127.0.0.1:8080）
timeout /t 2 /nobreak >nul

echo [2/3] Building (release) ...
cargo build --release -p gl-server
if errorlevel 1 (
    echo       Build failed, abort.
    exit /b 1
)

echo [3/3] Starting gl-server ...
set "CONFIG_PATH=%~dp0config.toml"
start "gl-server" /D "%~dp0" "%~dp0target\release\gl-server.exe"

echo.
echo ============================================================
echo  gl-server started in a new console window.
echo  Config: %CONFIG_PATH%
echo  Health: http://127.0.0.1:8080/health
echo ============================================================

endlocal
