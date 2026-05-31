@echo off
setlocal
set HOSTS=github.com 8.8.8.8 1.1.1.1
for %%h in (%HOSTS%) do (
    ping -n 1 -w 2000 %%h >nul
    if errorlevel 1 (echo %%h DOWN) else (echo %%h UP)
)
endlocal
