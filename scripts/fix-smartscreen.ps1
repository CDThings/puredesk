# PureDesk 启动问题自助修复（普通用户权限即可，无需管理员）
# 自动探测本机 PureDesk 安装位置，处理三类常见问题：
#   1. exe 被误打「低完整性」标签 → icacls 恢复为 Medium（找回桌面写入能力）
#   2. 网络下载标记（Mark-of-the-Web）→ Unblock-File 清除
#   3. 应用启动崩溃但找不到日志 → 把各候选日志位置打印出来
#
# 用法:  powershell -ExecutionPolicy Bypass -File fix-smartscreen.ps1 [-Exe <exe路径>]
#         -QuietAllSmartScreen 需管理员（会关闭全系统 SmartScreen，默认不执行）
param(
  [string]$Exe = "",
  [switch]$QuietAllSmartScreen
)

$ErrorActionPreference = "Continue"

function Find-PureDeskExe {
  if ($Exe -and (Test-Path $Exe)) { return $Exe }
  $candidates = @(
    (Join-Path $env:LOCALAPPDATA "Programs\PureDesk\puredesk.exe"),
    (Join-Path $env:USERPROFILE "PureDesk\puredesk.exe"),
    "E:\codespace\other\puredesk\src-tauri\target\release\puredesk.exe",
    "E:\codespace\other\puredesk\src-tauri\target\debug\puredesk.exe",
    "E:\PureDesk-Test\puredesk.exe"
  )
  foreach ($c in $candidates) {
    if (Test-Path $c) { return (Resolve-Path $c).Path }
  }
  return $null
}

$exe = Find-PureDeskExe
if ($null -eq $exe) {
  Write-Warning "找不到 puredesk.exe，请用 -Exe 指定路径"
  exit 1
}
Write-Output "=== 目标: $exe ==="

# 1) 清除网络标记 + 修复完整性标签（Medium）
Unblock-File $exe -ErrorAction SilentlyContinue
$parent = Split-Path $exe -Parent
# 标签可能挂在 exe 本身或其父目录（继承）；都处理一次
foreach ($t in @($exe, $parent)) {
  if (Test-Path $t) {
    icacls $t /setintegritylevel M *> $null
    $label = (icacls $t | Select-String "Mandatory").ToString().Trim()
    Write-Output ("完整性标签: $label")
  }
}

# 2) 定位日志（应用日志文件可能写在 exe 同目录 / TEMP / 用户目录）
$logCandidates = @(
  (Join-Path (Split-Path $exe -Parent) "puredesk.log"),
  (Join-Path $env:TEMP "com.puredesk.app\puredesk.log"),
  (Join-Path $env:TEMP "puredesk.log"),
  (Join-Path $env:USERPROFILE "puredesk.log")
)
Write-Output "=== 日志候选 ==="
foreach ($l in $logCandidates) {
  if (Test-Path $l) {
    Write-Output ("找到: " + $l)
    Get-Content $l -Tail 15
  } else {
    Write-Output ("(无) $l")
  }
}

# 3) （可选，需管理员）关闭 SmartScreen「检查应用和文件」
if ($QuietAllSmartScreen) {
  Write-Output "=== 关闭 SmartScreen（检查应用和文件）-> Off ==="
  Set-ItemProperty -Path "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer" `
    -Name SmartScreenEnabled -Value "Off" -Type String
  Write-Output ("SmartScreenEnabled = " +
    (Get-ItemProperty "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer").SmartScreenEnabled)
} else {
  Write-Output ""
  Write-Output "提示: 若要彻底消除未签名应用的 SmartScreen 弹窗，可用管理员执行:"
  Write-Output "  powershell -ExecutionPolicy Bypass -File $PSCommandPath -QuietAllSmartScreen"
  Write-Output "或手动: Windows 安全中心 → 基于声誉的保护 → 关闭「检查应用和文件」"
}
Write-Output "FIX_DONE"
