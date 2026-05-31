@echo off
setlocal
rem Backup script — copies sources/* to backups/<date>
set SRC=%~dp0sources
set DST=%~dp0backups\%date:~6,4%%date:~3,2%%date:~0,2%
if not exist "%DST%" mkdir "%DST%"
for /r "%SRC%" %%f in (*.txt *.csv *.log) do (
    echo Copying %%~nxf
    copy /Y "%%f" "%DST%\" >nul
)
echo Backup complete: %DST%
endlocal
