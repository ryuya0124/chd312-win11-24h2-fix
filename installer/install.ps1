$ErrorActionPreference = 'Stop'
$installRoot = Join-Path ([Environment]::GetFolderPath('ProgramFiles')) 'CHD312 Windows 11 Fix'
$userSid = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
$taskName = 'CHD312Win11Fix-' + $userSid
$scheduler = New-Object -ComObject 'Schedule.Service'
$scheduler.Connect()
$rootFolder = $scheduler.GetFolder('\')
$definition = $scheduler.NewTask(0)
$definition.RegistrationInfo.Description = 'CHD312 Windows 11 24H2+ Fix: MIDI に誤認された音声機能だけを USB Audio に戻します。'
$definition.Principal.UserId = $userSid
$definition.Principal.LogonType = 3
$definition.Principal.RunLevel = 1
$trigger = $definition.Triggers.Create(9)
$trigger.UserId = $userSid
$trigger.Delay = 'PT5S'
$action = $definition.Actions.Create(0)
$action.Path = Join-Path $installRoot 'chd312-background.exe'
$action.Arguments = 'watch'
$action.WorkingDirectory = $installRoot
$definition.Settings.Enabled = $true
$definition.Settings.Hidden = $true
$definition.Settings.AllowDemandStart = $true
$definition.Settings.StartWhenAvailable = $true
$definition.Settings.DisallowStartIfOnBatteries = $false
$definition.Settings.StopIfGoingOnBatteries = $false
$definition.Settings.ExecutionTimeLimit = 'PT0S'
$definition.Settings.MultipleInstances = 2
$definition.Settings.RestartCount = 3
$definition.Settings.RestartInterval = 'PT1M'
$null = $rootFolder.RegisterTaskDefinition($taskName, $definition, 6, $userSid, $null, 3)

$menuFolder = Join-Path ([Environment]::GetFolderPath('Programs')) 'CHD312 Windows 11 24H2+ Fix'
$null = New-Item -ItemType Directory -Path $menuFolder -Force
$shell = New-Object -ComObject WScript.Shell
$shortcut = $shell.CreateShortcut((Join-Path $menuFolder 'CHD312 Windows 11 24H2+ Fix.lnk'))
$shortcut.TargetPath = Join-Path $installRoot 'chd312-background.exe'
$shortcut.Arguments = 'start'
$shortcut.WorkingDirectory = $installRoot
$shortcut.Description = '画面を表示せずに CHD312 の自動修復監視を開始'
$shortcut.Save()
$shortcut = $shell.CreateShortcut((Join-Path $menuFolder 'Usage and commands.lnk'))
$shortcut.TargetPath = Join-Path $installRoot 'README.txt'
$shortcut.Save()
$shortcut = $shell.CreateShortcut((Join-Path $menuFolder 'Uninstall.lnk'))
$shortcut.TargetPath = Join-Path $installRoot 'Uninstall.exe'
$shortcut.Arguments = '--uninstall'
$shortcut.Save()

$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
$pathEntries = @($userPath -split ';' | Where-Object { $_ })
if ($pathEntries -notcontains $installRoot) {
    [Environment]::SetEnvironmentVariable('Path', (($pathEntries + $installRoot) -join ';'), 'User')
}
$uninstallKey = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\CHD312Win11Fix'
$null = New-Item -Path $uninstallKey -Force
$uninstallValues = @{
 DisplayName = 'CHD312 Windows 11 24H2+ Fix'
 DisplayVersion = '0.1.0'
 Publisher = 'ryuya0124'
 InstallLocation = $installRoot
 DisplayIcon = (Join-Path $installRoot 'chd312.exe')
 UninstallString = ('"' + (Join-Path $installRoot 'Uninstall.exe') + '" --uninstall')
 QuietUninstallString = ('"' + (Join-Path $installRoot 'Uninstall.exe') + '" --uninstall --silent')
 URLInfoAbout = 'https://github.com/ryuya0124/chd312-win11-24h2-fix'
}
$uninstallValues.GetEnumerator() | ForEach-Object { Set-ItemProperty -Path $uninstallKey -Name $_.Key -Value $_.Value }
New-ItemProperty -Path $uninstallKey -Name NoModify -PropertyType DWord -Value 1 -Force | Out-Null
New-ItemProperty -Path $uninstallKey -Name NoRepair -PropertyType DWord -Value 1 -Force | Out-Null
$rootFolder.GetTask($taskName).Run($null) | Out-Null
Write-Output 'Installation and task registration complete.'
