$ErrorActionPreference = 'Stop'
$installRoot = Join-Path ([Environment]::GetFolderPath('ProgramFiles')) 'CHD312 Windows 11 Fix'
$userSid = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
$taskName = 'CHD312Win11Fix-' + $userSid
$scheduler = New-Object -ComObject 'Schedule.Service'
$scheduler.Connect()
$rootFolder = $scheduler.GetFolder('\')
try { $registeredTask = $rootFolder.GetTask($taskName) } catch {
    if ($_.Exception.HResult -ne -2147024894) { throw }
    $registeredTask = $null
}
if ($registeredTask) {
    if ($registeredTask.State -eq 4) { $registeredTask.Stop(0) }
    $rootFolder.DeleteTask($taskName, 0)
}
$menuFolder = Join-Path ([Environment]::GetFolderPath('Programs')) 'CHD312 Windows 11 24H2+ Fix'
foreach ($shortcutName in @('CHD312 Windows 11 24H2+ Fix.lnk', 'Usage and commands.lnk', 'Uninstall.lnk', '使い方・コマンド一覧.lnk', 'アンインストール.lnk')) {
    $shortcutPath = Join-Path $menuFolder $shortcutName
    if (Test-Path -LiteralPath $shortcutPath) { Remove-Item -LiteralPath $shortcutPath -Force }
}
if ((Test-Path -LiteralPath $menuFolder) -and @(Get-ChildItem -LiteralPath $menuFolder -Force).Count -eq 0) {
    Remove-Item -LiteralPath $menuFolder -Force
}
$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
[Environment]::SetEnvironmentVariable('Path', ((@($userPath -split ';' | Where-Object { $_ -and $_.TrimEnd('\') -ine $installRoot }) -join ';')), 'User')
$uninstallKey = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\CHD312Win11Fix'
if (Test-Path -LiteralPath $uninstallKey) { Remove-Item -LiteralPath $uninstallKey -Recurse -Force }
Write-Output 'Task, shortcuts, PATH and uninstall registration removed. Audio drivers are preserved.'
