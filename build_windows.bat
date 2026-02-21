@echo off
echo AutoGit Build Script for Windows
echo.

cargo build --release

if %ERRORLEVEL% EQU 0 (
    echo.
    echo [V] Build successful!
    echo Binary is located at: target\release\autogit.exe
) else (
    echo.
    echo [X] Build failed. Please ensure Rust is installed.
)
pause
