# Synthetic drag experiment: drag a desktop icon (real mouse input) onto a PureDesk box
$ErrorActionPreference = "Continue"
Add-Type -TypeDefinition @"
using System;
using System.Text;
using System.Runtime.InteropServices;

public class DragHelper {
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowW(string cls, string title);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowExW(IntPtr parent, IntPtr after, string cls, string title);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("kernel32.dll")] public static extern IntPtr OpenProcess(uint access, bool inherit, uint pid);
  [DllImport("kernel32.dll")] public static extern IntPtr VirtualAllocEx(IntPtr h, IntPtr addr, UIntPtr size, uint type, uint protect);
  [DllImport("kernel32.dll")] public static extern bool VirtualFreeEx(IntPtr h, IntPtr addr, UIntPtr size, uint type);
  [DllImport("kernel32.dll")] public static extern bool WriteProcessMemory(IntPtr h, IntPtr addr, IntPtr buf, UIntPtr size, out IntPtr written);
  [DllImport("kernel32.dll")] public static extern bool ReadProcessMemory(IntPtr h, IntPtr addr, IntPtr buf, UIntPtr size, out IntPtr read);
  [DllImport("kernel32.dll")] public static extern bool CloseHandle(IntPtr h);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr SendMessageTimeoutW(IntPtr h, uint msg, IntPtr wp, IntPtr lp, uint flags, uint timeout, out IntPtr result);
  [DllImport("user32.dll")] public static extern uint SendInput(uint n, INPUT[] inputs, int size);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);

  public const uint LVM_FIRST = 0x1000;
  public const uint LVM_GETITEMCOUNT = LVM_FIRST + 4;
  public const uint LVM_GETITEMPOSITION = LVM_FIRST + 16;
  public const uint LVM_GETITEMTEXTW = LVM_FIRST + 115;

  [StructLayout(LayoutKind.Sequential)]
  public struct LVITEMW {
    public uint mask; public int iItem; public int iSubItem; public uint state; public uint stateMask;
    public IntPtr pszText; public int cchTextMax; public int iImage; public IntPtr lParam; public int iIndent;
  }

  public static string GetItemText(IntPtr lv, int index) {
    uint pid; GetWindowThreadProcessId(lv, out pid);
    IntPtr proc = OpenProcess(0x1F0FFF, false, pid); // PROCESS_ALL_ACCESS
    if (proc == IntPtr.Zero) return "";
    IntPtr remote = VirtualAllocEx(proc, IntPtr.Zero, (UIntPtr)4096, 0x3000, 0x04);
    if (remote == IntPtr.Zero) { CloseHandle(proc); return ""; }
    try {
      LVITEMW item = new LVITEMW();
      item.mask = 1; item.iItem = index; item.iSubItem = 0;
      item.pszText = (IntPtr)((long)remote + 80); item.cchTextMax = 512;
      IntPtr ptr = Marshal.AllocHGlobal(Marshal.SizeOf(typeof(LVITEMW)));
      Marshal.StructureToPtr(item, ptr, false);
      IntPtr w; WriteProcessMemory(proc, remote, ptr, (UIntPtr)Marshal.SizeOf(typeof(LVITEMW)), out w);
      IntPtr res;
      SendMessageTimeoutW(lv, LVM_GETITEMTEXTW, (IntPtr)index, remote, 0x0008, 2000, out res);
      byte[] buf = new byte[512];
      GCHandle g = GCHandle.Alloc(buf, GCHandleType.Pinned);
      ReadProcessMemory(proc, (IntPtr)((long)remote + 80), g.AddrOfPinnedObject(), (UIntPtr)512, out w);
      g.Free();
      Marshal.FreeHGlobal(ptr);
      return Encoding.Unicode.GetString(buf).TrimEnd('\0');
    } finally { VirtualFreeEx(proc, remote, (UIntPtr)0, 0x8000); CloseHandle(proc); }
  }

  public static int GetItemCount(IntPtr lv) {
    IntPtr res; SendMessageTimeoutW(lv, LVM_GETITEMCOUNT, IntPtr.Zero, IntPtr.Zero, 0x0008, 2000, out res);
    return (int)res;
  }

  public static bool GetItemPos(IntPtr lv, int index, out int x, out int y) {
    x = 0; y = 0;
    uint pid; GetWindowThreadProcessId(lv, out pid);
    IntPtr proc = OpenProcess(0x1F0FFF, false, pid);
    if (proc == IntPtr.Zero) return false;
    IntPtr remote = VirtualAllocEx(proc, IntPtr.Zero, (UIntPtr)16, 0x3000, 0x04);
    if (remote == IntPtr.Zero) { CloseHandle(proc); return false; }
    try {
      IntPtr res;
      SendMessageTimeoutW(lv, LVM_GETITEMPOSITION, (IntPtr)index, remote, 0x0008, 2000, out res);
      byte[] buf = new byte[8];
      GCHandle g = GCHandle.Alloc(buf, GCHandleType.Pinned);
      IntPtr w;
      ReadProcessMemory(proc, remote, g.AddrOfPinnedObject(), (UIntPtr)8, out w);
      g.Free();
      x = BitConverter.ToInt32(buf, 0); y = BitConverter.ToInt32(buf, 4);
      return true;
    } finally { VirtualFreeEx(proc, remote, (UIntPtr)0, 0x8000); CloseHandle(proc); }
  }

  [StructLayout(LayoutKind.Sequential)]
  public struct MOUSEINPUT { public int dx; public int dy; public uint mouseData; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
  [StructLayout(LayoutKind.Explicit)]
  public struct INPUT { [FieldOffset(0)] public uint type; [FieldOffset(8)] public MOUSEINPUT mi; }

  public static void Mouse(string kind, int x, int y) {
    const uint MOVE = 0x0001, ABS = 0x8000, LEFTDOWN = 0x0002, LEFTUP = 0x0004;
    uint flags = 0;
    if (kind == "move") flags = MOVE | ABS;
    if (kind == "down") flags = LEFTDOWN | MOVE | ABS;
    if (kind == "up") flags = LEFTUP | MOVE | ABS;
    INPUT[] inp = new INPUT[1];
    inp[0].type = 0;
    inp[0].mi.dx = (int)((x + 0.5) * 65536 / 1920);
    inp[0].mi.dy = (int)((y + 0.5) * 65536 / 1080);
    inp[0].mi.dwFlags = flags;
    SendInput(1, inp, Marshal.SizeOf(typeof(INPUT)));
  }
}
"@

# --- locate desktop listview ---
$progman = [DragHelper]::FindWindowW("Progman", "Program Manager")
$defview = [DragHelper]::FindWindowExW($progman, [IntPtr]::Zero, "SHELLDLL_DefView", $null)
if ($defview -eq [IntPtr]::Zero) {
  # sometimes SHELLDLL_DefView lives under a WorkerW
  $worker = [IntPtr]::Zero
  do {
    $worker = [DragHelper]::FindWindowExW([IntPtr]::Zero, $worker, "WorkerW", $null)
    if ($worker -ne [IntPtr]::Zero) { $defview = [DragHelper]::FindWindowExW($worker, [IntPtr]::Zero, "SHELLDLL_DefView", $null) }
  } while ($worker -ne [IntPtr]::Zero -and $defview -eq [IntPtr]::Zero)
}
$lv = [DragHelper]::FindWindowExW($defview, [IntPtr]::Zero, "SysListView32", "FolderView")
Write-Output ("LISTVIEW=" + ($lv -ne [IntPtr]::Zero))

# --- create test file, find its icon ---
$desktop = [Environment]::GetFolderPath("Desktop")
$testFile = Join-Path $desktop "drag-test.txt"
[System.IO.File]::WriteAllText($testFile, "puredesk drag experiment", [System.Text.Encoding]::UTF8)
Start-Sleep -Seconds 2

$count = [DragHelper]::GetItemCount($lv)
Write-Output ("ITEMS=" + $count)
$found = -1; $ix = 0; $iy = 0
for ($i = 0; $i -lt $count; $i++) {
  $t = [DragHelper]::GetItemText($lv, $i)
  if ($t -like "drag-test*") {
    if ([DragHelper]::GetItemPos($lv, $i, [ref]$ix, [ref]$iy)) { $found = $i }
    break
  }
}
Write-Output ("FOUND=" + $found + " POS=" + $ix + "," + $iy)
if ($found -lt 0) { Write-Output "RESULT=test icon not found; abort"; Remove-Item $testFile -Force -ErrorAction SilentlyContinue; exit }

# --- target: first visible box center from exe-dir config ---
$logPath = "E:\PureDesk-Test\puredesk.log"
$cfg = Get-Content "C:\Users\mozzie\AppData\Roaming\com.puredesk.app\config.json" -Raw -Encoding UTF8 | ConvertFrom-Json
$box = $cfg.boxes | Where-Object { $_.visible } | Select-Object -First 1
if (-not $box) { Write-Output "RESULT=no visible box; abort"; Remove-Item $testFile -Force -ErrorAction SilentlyContinue; exit }
$tx = [int]($box.x + $box.w / 2); $ty = [int]($box.y + 30)
Write-Output ("TARGET=" + $tx + "," + $ty + " box=" + $box.id)

# --- expose desktop, then synthetic drag ---
$shell = New-Object -ComObject Shell.Application
$shell.MinimizeAll()
Start-Sleep -Seconds 2
# re-show PureDesk windows (boxes got minimized as well)
Add-Type -TypeDefinition @"
using System; using System.Runtime.InteropServices;
public class WinShow {
  public delegate bool EnumProc(IntPtr h, IntPtr lp);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr lp);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  public static void ShowAllOf(uint pid) {
    EnumWindows((h, lp) => {
      uint p; GetWindowThreadProcessId(h, out p);
      if (p == pid) { ShowWindow(h, 9); }
      return true;
    }, IntPtr.Zero);
  }
}
"@
$pid0 = (Get-Process puredesk, puredesk-user -ErrorAction SilentlyContinue | Select-Object -First 1).Id
[WinShow]::ShowAllOf([uint32]$pid0)
Start-Sleep -Seconds 1
Add-Type -AssemblyName System.Windows.Forms,System.Drawing
$b2 = New-Object System.Drawing.Bitmap 1920, 1080
$g2 = [System.Drawing.Graphics]::FromImage($b2)
$g2.CopyFromScreen(0, 0, 0, 0, $b2.Size)
$b2.Save("E:\codespace\other\puredesk\pre-drag.png")
$g2.Dispose(); $b2.Dispose()
Write-Output "PRE_DRAG_SHOT=ok"
[DragHelper]::Mouse("move", $ix + 10, $iy + 10); Start-Sleep -Milliseconds 200
[DragHelper]::Mouse("down", $ix + 10, $iy + 10); Start-Sleep -Milliseconds 250
for ($s = 1; $s -le 8; $s++) {
  $cx = $ix + [int](($tx - $ix) * $s / 8); $cy = $iy + [int](($ty - $iy) * $s / 8)
  [DragHelper]::Mouse("move", $cx, $cy); Start-Sleep -Milliseconds 60
}
Start-Sleep -Milliseconds 400
Add-Type -AssemblyName System.Windows.Forms,System.Drawing
$b3 = New-Object System.Drawing.Bitmap 1920, 1080
$g3 = [System.Drawing.Graphics]::FromImage($b3)
$g3.CopyFromScreen(0, 0, 0, 0, $b3.Size)
$b3.Save("E:\codespace\other\puredesk\mid-drag.png")
$g3.Dispose(); $b3.Dispose()
Write-Output "MID_DRAG_SHOT=ok"
[DragHelper]::Mouse("up", $tx, $ty); Start-Sleep -Seconds 3
Write-Output "DRAG_DONE"
$after = 0
if (Test-Path $logPath) { $after = (Get-Content $logPath -Encoding UTF8 | Select-String "OLE Drop").Count }
if ($after -gt $before) { $dragWorked = $true; Write-Output "ATTEMPT $attempt : OLE Drop seen" }
else { Write-Output "ATTEMPT $attempt : no OLE Drop, retrying" }
$shell.UndoMinimizeALL()
Start-Sleep -Milliseconds 500
Write-Output "WINDOWS_RESTORED"

# --- verify: log events + file landing ---
$log = Get-Content "C:\Users\mozzie\AppData\Roaming\com.puredesk.app\puredesk.log" -ErrorAction SilentlyContinue | Select-String "puredesk-drag-drop|OLE Drop" | Select-Object -Last 5
Write-Output ("LOG_EVENTS=" + ($log | Measure-Object).Count)
$log | ForEach-Object { Write-Output $_ }
$storage = Join-Path $desktop ([string][char]0x684C + [char]0x9762 + [char]0x6574 + [char]0x7406)
$landed = Test-Path (Join-Path (Join-Path $storage "应用") "drag-test.txt")
Write-Output ("LANDED_IN_STORAGE=" + $landed)
if (-not (Test-Path $testFile)) { Write-Output "FILE_LEFT_DESKTOP=yes" }
Write-Output "DONE"
