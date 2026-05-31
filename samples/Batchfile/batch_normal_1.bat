@echo off
net session >nul 2>&1
if not %errorLevel% == 0 (
    echo Requesting admin privileges...
    powershell -Command "Start-Process '%~f0' -Verb runAs"
    exit /b
)
echo Running as administrator.
sc config "HKUOBSFJ" start= auto
