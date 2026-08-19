@echo off
setlocal EnableExtensions DisableDelayedExpansion

set "ECLIPSE_INSTALL_ROOT=%~dp0"
set "ECLIPSE_BASH="
set "ECLIPSE_BOOTSTRAP_YES=false"
set "ECLIPSE_BOOTSTRAP_DEMO=false"

for %%A in (%*) do (
    if /I "%%~A"=="--yes" set "ECLIPSE_BOOTSTRAP_YES=true"
    if /I "%%~A"=="--demo" set "ECLIPSE_BOOTSTRAP_DEMO=true"
)

call :find_git_bash
if defined ECLIPSE_BASH goto launch

echo Eclipse Setup needs Git for Windows to start its interactive installer.

if "%ECLIPSE_BOOTSTRAP_DEMO%"=="true" (
    echo Demo mode will not install Git or make any system changes.
    echo Install Git for Windows from https://git-scm.com/download/win, then run:
    echo   install.cmd --demo
    exit /b 1
)

where winget >nul 2>nul
if errorlevel 1 (
    echo WinGet is not available, so Git cannot be installed automatically.
    echo Install Git for Windows from https://git-scm.com/download/win, then run:
    echo   install.cmd
    exit /b 1
)

if not "%ECLIPSE_BOOTSTRAP_YES%"=="true" call :confirm_git_install
if errorlevel 1 exit /b 1

echo Installing Git for Windows. Windows may request administrator approval.
call winget install --id Git.Git --exact --source winget --accept-package-agreements --accept-source-agreements --disable-interactivity
if errorlevel 1 (
    echo Git for Windows could not be installed.
    echo Resolve the WinGet error above, then run: install.cmd
    exit /b 1
)

call :find_git_bash
if not defined ECLIPSE_BASH (
    echo Git was installed, but Git Bash could not be located yet.
    echo Open a new CMD or PowerShell window, then run: install.cmd
    exit /b 1
)

:launch
"%ECLIPSE_BASH%" "%ECLIPSE_INSTALL_ROOT%install.sh" %*
exit /b %ERRORLEVEL%

:find_git_bash
if defined ECLIPSE_BASH_EXE if exist "%ECLIPSE_BASH_EXE%" set "ECLIPSE_BASH=%ECLIPSE_BASH_EXE%"
if defined ECLIPSE_BASH exit /b 0
if /I "%ECLIPSE_SKIP_GIT_DISCOVERY%"=="1" exit /b 0

for %%B in (
    "%ProgramFiles%\Git\bin\bash.exe"
    "%ProgramW6432%\Git\bin\bash.exe"
    "%LocalAppData%\Programs\Git\bin\bash.exe"
) do if exist "%%~B" set "ECLIPSE_BASH=%%~B"
if defined ECLIPSE_BASH exit /b 0

for %%K in (
    "HKCU\Software\GitForWindows"
    "HKLM\Software\GitForWindows"
    "HKLM\Software\WOW6432Node\GitForWindows"
) do call :find_git_bash_in_registry %%~K
exit /b 0

:find_git_bash_in_registry
if defined ECLIPSE_BASH exit /b 0
for /f "tokens=2,*" %%A in ('reg query "%~1" /v InstallPath 2^>nul ^| findstr /I "InstallPath"') do (
    if exist "%%B\bin\bash.exe" set "ECLIPSE_BASH=%%B\bin\bash.exe"
)
exit /b 0

:confirm_git_install
set "ECLIPSE_GIT_REPLY="
set /p "ECLIPSE_GIT_REPLY=Install Git for Windows now? [Y/n] "
if /I "%ECLIPSE_GIT_REPLY%"=="n" exit /b 1
if /I "%ECLIPSE_GIT_REPLY%"=="no" exit /b 1
exit /b 0
