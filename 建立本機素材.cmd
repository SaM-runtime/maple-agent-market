@echo off
chcp 65001 >nul
setlocal
echo Maple Agent Market 將從 Maple Atelier 與 maplestory.io 下載角色渲染，
echo 並只在此資料夾的 private-assets 內建立本機素材。
echo 下載結果不是本專案 MIT 授權的一部分，請勿重新上傳或打包散布。
echo.
choice /C YN /N /M "同意以上說明並繼續？ [Y/N] "
if errorlevel 2 exit /b 0
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%~dp0tools\Bootstrap-MapleLocalAssets.ps1" -ProjectRoot "%~dp0" -AcceptThirdPartyAssetNotice -IncludeClassicSkills
set "BOOTSTRAP_EXIT=%ERRORLEVEL%"
echo.
if not "%BOOTSTRAP_EXIT%"=="0" echo 建立失敗，錯誤代碼：%BOOTSTRAP_EXIT%
if "%BOOTSTRAP_EXIT%"=="0" echo 建立完成。請依 README 使用 private-assets\skins\catalog-pack 啟動。
pause
exit /b %BOOTSTRAP_EXIT%
