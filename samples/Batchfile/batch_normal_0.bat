@echo off
rem Add a directory to PATH if it isn't already there
set NEW=%~1
if "%NEW%"=="" (
    echo Usage: %~nx0 ^<directory^>
    exit /b 1
)
echo %PATH% | findstr /i /c:"%NEW%" >nul
if not errorlevel 1 (
    echo Already on PATH.
    exit /b 0
)
setx PATH "%PATH%;%NEW%"
echo Added %NEW% to PATH.
