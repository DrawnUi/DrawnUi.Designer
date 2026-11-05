# Restarts DesignerSample under dotnet watch with the designer agent attached (no project-file
# edit), and brings its window to the front. Exactly one watch runs on the sample: the one this
# script started last (its pid is kept in %TEMP%), plus any watch started with the designer targets,
# is stopped with its whole process tree first.
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
$sample = Join-Path $repo 'samples\wpf\DesignerSample'
$agent = Join-Path $repo 'agents\dotnet\Agent\bin\Release\net8.0-windows\DrawnUi.Designer.Agent.dll'
$targets = Join-Path $repo 'agents\dotnet\DrawnUi.Designer.targets'
$pidFile = Join-Path $env:TEMP 'designer-sample-watch.pid'
$log = Join-Path $env:TEMP 'designer-sample-watch.log'

function Stop-Tree([int]$id) {
    Get-CimInstance Win32_Process -Filter "ParentProcessId=$id" | ForEach-Object { Stop-Tree $_.ProcessId }
    Stop-Process -Id $id -ErrorAction SilentlyContinue
}

$old = @()
if (Test-Path $pidFile) { $old += [int](Get-Content $pidFile) }
$old += Get-CimInstance Win32_Process -Filter "Name='dotnet.exe'" |
    Where-Object { $_.CommandLine -match 'watch run' -and $_.CommandLine -match [regex]::Escape($targets) } |
    Select-Object -ExpandProperty ProcessId
$old | Sort-Object -Unique | ForEach-Object { Stop-Tree $_ }
Get-Process DesignerSample -ErrorAction SilentlyContinue | Where-Object { $_.Path -like "$sample*" } | Stop-Process
Start-Sleep 1

# The agent from the current source (the sample held the old DLL until now).
& dotnet build (Join-Path $repo 'agents\dotnet\Agent\DrawnUi.Designer.Agent.csproj') -c Release -v q | Out-Null
if ($LASTEXITCODE -ne 0) { throw "agent build failed" }

$env:DRAWNUI_DESIGNER_AGENT = $agent
$watch = Start-Process dotnet -ArgumentList 'watch', 'run', '--non-interactive', "--property:CustomAfterMicrosoftCommonTargets=$targets" `
    -WorkingDirectory $sample -WindowStyle Hidden -RedirectStandardOutput $log -PassThru
Remove-Item Env:DRAWNUI_DESIGNER_AGENT
Set-Content $pidFile $watch.Id

$deadline = (Get-Date).AddMinutes(4)
do {
    $app = Get-Process DesignerSample -ErrorAction SilentlyContinue | Where-Object { $_.Path -like "$sample*" } | Select-Object -First 1
    if ($app) { break }
    Start-Sleep 1
} while ((Get-Date) -lt $deadline)
if (-not $app) { throw "DesignerSample did not start; see $log" }
& (Join-Path $PSScriptRoot 'front.ps1') -ProcessId $app.Id
"sample pid $($app.Id) (watch $($watch.Id)), log $log"
