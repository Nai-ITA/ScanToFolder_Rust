@echo off
rem ================================================================
rem  Scan to Folder 2.0 (Rust) per Windows
rem  ->  dist\ScanToFolder.exe   (un solo file, niente Python)
rem  Requisiti: Rust (rustup + Build Tools di Visual Studio)
rem  Installer NSIS opzionale:  cargo install tauri-cli  poi  build.bat installer
rem ================================================================
setlocal
cd /d "%~dp0"
where cargo >nul 2>nul || ( echo Rust non installato: https://rustup.rs & pause & exit /b 1 )
pushd src-tauri
if /i "%~1"=="installer" (
    cargo tauri build --bundles nsis || ( popd & pause & exit /b 1 )
) else (
    cargo build --release || ( popd & pause & exit /b 1 )
)
popd
if not exist dist mkdir dist
copy /y src-tauri\target\release\ScanToFolder.exe dist\ScanToFolder.exe >nul
if /i "%~1"=="installer" copy /y src-tauri\target\release\bundle\nsis\*.exe dist\ >nul
echo.
echo Pronto: dist\ScanToFolder.exe
pause
