<#
.SYNOPSIS
  EduGuard Windows installer (unattended-friendly). Expects the matching
  eduguard-server.exe / eduguard-agent.exe next to this script.

.EXAMPLE
  .\install.ps1 -Component server
  .\install.ps1 -Component agent -ServerUrl http://server:8443 -Token eg-...
  .\install.ps1 -Component agent -Uninstall
#>
param(
  [Parameter(Mandatory = $true)]
  [ValidateSet("server", "agent")]
  [string]$Component,

  [string]$ServerUrl,
  [string]$Token,
  [switch]$Uninstall
)
$ErrorActionPreference = "Stop"

$exeName = "eduguard-$Component.exe"
$src     = Join-Path $PSScriptRoot $exeName
$destDir = Join-Path $env:ProgramFiles "EduGuard\$Component"
$dest    = Join-Path $destDir $exeName

if ($Uninstall) {
  if ($Component -eq "server") {
    schtasks /End    /TN "EduGuard Server" 2>$null | Out-Null
    schtasks /Delete /F /TN "EduGuard Server" 2>$null | Out-Null
    netsh advfirewall firewall delete rule name="EduGuard Server" | Out-Null
  } else {
    Stop-Process -Name "eduguard-agent" -Force -ErrorAction SilentlyContinue
    Remove-ItemProperty -Path "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Run" `
      -Name "EduGuardAgent" -ErrorAction SilentlyContinue
  }
  Remove-Item $destDir -Recurse -Force -ErrorAction SilentlyContinue
  Write-Host "EduGuard $Component uninstalled (data kept in C:\ProgramData\EduGuard)."
  return
}

if (-not (Test-Path $src)) { throw "expected $exeName next to install.ps1" }
New-Item -ItemType Directory -Force -Path $destDir | Out-Null
Copy-Item $src $dest -Force

if ($Component -eq "server") {
  # First run creates C:\ProgramData\EduGuard\server\server.toml and prints
  # the generated console admin password.
  Copy-Item (Join-Path $PSScriptRoot "start-server.cmd") $destDir -Force
  $pw = (& $dest reset-admin-password | Select-Object -Last 1).Trim()

  # The server is a console app (not a Windows service): run it at boot as
  # SYSTEM via a scheduled task, plus a firewall rule for port 8443.
  schtasks /Create /F /TN "EduGuard Server" /SC ONSTART /RU SYSTEM `
    /TR "`"$destDir\start-server.cmd`"" | Out-Null
  schtasks /Run /TN "EduGuard Server" | Out-Null
  netsh advfirewall firewall add rule name="EduGuard Server" dir=in action=allow protocol=TCP localport=8443 | Out-Null

  Write-Host ""
  Write-Host "EduGuard Server installed."
  Write-Host "  Console:   http://localhost:8443/"
  Write-Host "  Username:  admin"
  Write-Host "  Password:  $pw"
  Write-Host "  (stored in C:\ProgramData\EduGuard\server\server.toml; rotate with 'eduguard-server reset-admin-password')"
  Write-Host "  Enrollment token: eduguard-server new-enrollment-token --label classroom"
} else {
  # The agent must run in the interactive user session (tray icon + overlay),
  # so start it at every logon via the HKLM Run key.
  Set-ItemProperty -Path "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Run" `
    -Name "EduGuardAgent" -Value "`"$dest`" run"

  if ($ServerUrl -and $Token) {
    & $dest enroll --server $ServerUrl --token $Token
  } else {
    Write-Host "Enroll this device:  eduguard-agent enroll --server http://SERVER:8443 --token eg-..."
  }
  Start-Process $dest -ArgumentList "run"
  Write-Host ""
  Write-Host "EduGuard Agent installed (starts at each user logon; tray icon in the notification area)."
}
