# PureDesk 用户安装脚本（在任意 PowerShell 中运行，无需管理员）
# 用法:  powershell -ExecutionPolicy Bypass -File install.ps1 [-Source <exe路径>] [-ScheduledTask]
# 功能:  复制 exe 到独立目录(脱离构建沙箱的低完整性标签)、解锁、创建桌面快捷方式
# -ScheduledTask 额外注册计划任务启动器（仅在你明确需要时开启；默认不注册，避免在系统留下计划任务）
param(
  [string]$Source = "$PSScriptRoot\..\src-tauri\target\release\puredesk.exe",
  [switch]$ScheduledTask
)
$ErrorActionPreference = "Stop"

$Source = [System.IO.Path]::GetFullPath($Source)
if (-not (Test-Path $Source)) {
  Write-Error "找不到 $Source ，请先用 'npm run tauri build' 构建，或用 -Source 指定路径"
  exit 1
}

# 安装目录：优先用户程序目录（若被策略拒绝则退回用户主目录）
$destDir = "$env:LOCALAPPDATA\Programs\PureDesk"
try {
  New-Item -ItemType Directory -Force -Path $destDir | Out-Null
  $probe = Join-Path $destDir ".write-test"
  [System.IO.File]::WriteAllText($probe, "x")
  Remove-Item $probe -Force
} catch {
  $destDir = "$env:USERPROFILE\PureDesk"
  New-Item -ItemType Directory -Force -Path $destDir | Out-Null
}
$dest = Join-Path $destDir "puredesk.exe"

# 关闭运行中的实例后复制
Get-Process puredesk -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
Start-Sleep -Seconds 1
Copy-Item $Source $dest -Force

# 解除网络标记（若有）+ 确保完整性标签为 Medium（修复构建沙箱带来的低完整性标签）
Unblock-File $dest -ErrorAction SilentlyContinue
icacls $dest /setintegritylevel M | Out-Null

# 桌面快捷方式
$ws = New-Object -ComObject WScript.Shell
$lnk = $ws.CreateShortcut("$([Environment]::GetFolderPath('Desktop'))\PureDesk.lnk")
$lnk.TargetPath = $dest
$lnk.WorkingDirectory = $destDir
$lnk.IconLocation = $dest
$lnk.Save()

# 可选：注册计划任务启动器（仅显式要求时；避免在系统留下计划任务）
if ($ScheduledTask) {
  schtasks /Create /TN "PureDesk" /TR "`"$dest`"" /SC DAILY /ST 00:00 /F | Out-Null
  Write-Output "已注册计划任务 PureDesk（备用启动: schtasks /run /tn PureDesk）"
}

Write-Output "✅ 已安装到 $destDir"
Write-Output "   桌面快捷方式: PureDesk.lnk"
Start-Process $dest
