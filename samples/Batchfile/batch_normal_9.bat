@echo off
where /q git || (echo git not on PATH & exit /b 1)
git rev-parse --is-inside-work-tree >nul 2>&1 || (echo not a git repo & exit /b 1)
for /f "delims=" %%b in ('git rev-parse --abbrev-ref HEAD') do set BRANCH=%%b
echo Current branch: %BRANCH%
