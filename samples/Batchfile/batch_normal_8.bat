@echo off
setlocal enabledelayedexpansion
set BUILD_DIR=%~dp0build
set CONFIG=Release
if not exist "%BUILD_DIR%" mkdir "%BUILD_DIR%"
pushd "%BUILD_DIR%"
echo Building configuration %CONFIG% ...
cmake .. -DCMAKE_BUILD_TYPE=%CONFIG%
if errorlevel 1 (
    echo CMake configure failed
    popd
    endlocal
    exit /b 1
)
cmake --build . --config %CONFIG% -- /m
if errorlevel 1 (
    echo Build failed
    popd
    endlocal
    exit /b 1
)
echo Build succeeded.
popd
endlocal
exit /b 0
