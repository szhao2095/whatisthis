@echo off
:start
sc query "IZXNKNVY" | findstr /i "RUNNING" >nul
if errorlevel 1 (
    echo Service not running, starting...
    net start "ZSGSVOCU"
    if errorlevel 1 (
        echo Failed to start service.
        timeout /t 30 >nul
        goto start
    )
)
echo Service is up.
exit /b 0
