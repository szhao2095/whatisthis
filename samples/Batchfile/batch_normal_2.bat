@echo off
setlocal
set TMPLOG=%TEMP%\install-%RANDOM%.log
echo Logging to %TMPLOG%
(
    echo Installer started at %date% %time%
    echo User: %USERNAME%
    echo Host: %COMPUTERNAME%
) > "%TMPLOG%"
type "%TMPLOG%"
endlocal
