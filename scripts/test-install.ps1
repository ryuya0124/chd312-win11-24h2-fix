# Run only on an isolated Windows build runner, never as an ordinary build step on a user's PC.
$ErrorActionPreference = 'Stop'
$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) { throw 'Installer smoke test requires an elevated disposable Windows runner.' }
$installRoot = Join-Path ([Environment]::GetFolderPath('ProgramFiles')) 'CHD312 Windows 11 Fix'
if (Test-Path -LiteralPath $installRoot) { throw 'Existing install detected; refusing destructive smoke test.' }
$taskName = 'CHD312Win11Fix-' + [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
$setupPath = (Resolve-Path -LiteralPath 'dist/chd312-setup-x64.exe').Path
$setupProcess = Start-Process -FilePath $setupPath -ArgumentList '--silent' -WindowStyle Hidden -PassThru -Wait
if ($setupProcess.ExitCode -ne 0) {
    Get-Content -LiteralPath (Join-Path $env:LOCALAPPDATA 'CHD312Win11Fix\fix.log') -ErrorAction SilentlyContinue | Write-Output
    throw ('Installation failed, exit code ' + $setupProcess.ExitCode)
}
try {
    foreach ($name in @('chd312.exe','chd312-background.exe','Uninstall.exe','README.txt')) {
        if (-not (Test-Path -LiteralPath (Join-Path $installRoot $name))) { throw ('Missing installed file: ' + $name) }
    }
    $task = Get-ScheduledTask -TaskName $taskName
    if ($task.Principal.RunLevel -ne 'Highest') { throw 'Task is not configured for elevation.' }
    $cli = Join-Path $installRoot 'chd312.exe'
    $ready = $false
    for ($i=0; $i -lt 40; $i++) {
        $state = (& $cli status --json | ConvertFrom-Json).status
        if ($state.running) { $ready=$true; break }
        Start-Sleep -Milliseconds 250
    }
    if (-not $ready) { throw 'Scheduled background monitor did not start.' }
    $backgroundProcess = Get-Process -Id $state.pid
    if ($backgroundProcess.MainWindowHandle -ne 0) { throw 'Background monitor created a window.' }
    & $cli disable
    if ($LASTEXITCODE) { throw 'Disable command failed.' }
    $state = (& $cli status --json | ConvertFrom-Json).status
    if ($state.automatic) { throw 'Disable did not persist.' }
    & $cli enable
    if ($LASTEXITCODE) { throw 'Enable command failed.' }
    & $cli apply --force
    if ($LASTEXITCODE -eq 0) { throw 'Expected a refusal when no physical matching CHD312 exists on the runner.' }
    & $cli stop
    if ($LASTEXITCODE) { throw 'Stop command failed.' }
    & $cli start
    if ($LASTEXITCODE) { throw 'Start command failed.' }
} finally {
    $uninstaller = Join-Path $installRoot 'Uninstall.exe'
    if (Test-Path -LiteralPath $uninstaller) {
        $uninstallProcess = Start-Process -FilePath $uninstaller -ArgumentList '--uninstall --silent' -WindowStyle Hidden -PassThru -Wait
        if ($uninstallProcess.ExitCode -ne 0) { throw 'Uninstallation failed.' }
    }
}
for ($i=0;$i -lt 40 -and (Test-Path -LiteralPath $installRoot);$i++) { Start-Sleep -Milliseconds 500 }
if (Test-Path -LiteralPath $installRoot) { throw 'Installation directory remains after uninstall.' }
if (Get-ScheduledTask -TaskName $taskName -ErrorAction SilentlyContinue) { throw 'Scheduled task remains after uninstall.' }
if (Test-Path -LiteralPath 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\CHD312Win11Fix') { throw 'Uninstall registration remains.' }
Write-Output 'Install, hidden task, CLI control and uninstall smoke tests passed.'
