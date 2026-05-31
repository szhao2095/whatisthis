@echo off
setlocal enabledelayedexpansion
set COUNT=0
set TOTAL=0
for %%f in (*.log) do (
    set /a COUNT+=1
    for %%a in ("%%f") do set /a TOTAL+=%%~za
)
echo Found !COUNT! files, total size !TOTAL! bytes.
endlocal
