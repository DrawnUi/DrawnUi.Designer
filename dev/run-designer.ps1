# Rebuilds the DrawnUI Designer, closes the running one (only this repo's exe), starts the new one
# and brings its window to the front. Run it after every designer change that someone is testing.
param([switch]$Release)
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
$config = if ($Release) { 'release' } else { 'debug' }
$exe = Join-Path $repo "target\$config\drawnui-designer.exe"

Get-Process drawnui-designer -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $exe } | Stop-Process
Start-Sleep -Milliseconds 500

Push-Location $repo
try {
    $args = @('build', '-p', 'designer-desktop')
    if ($Release) { $args += '--release' }
    & cargo @args
    if ($LASTEXITCODE -ne 0) { throw "build failed" }
    # The .NET agent apps get when the designer runs them (its targets take this repo's build).
    & dotnet build agents\dotnet\Agent -c Release -v q -nologo | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "agent build failed" }
} finally { Pop-Location }

$p = Start-Process $exe -WorkingDirectory (Split-Path $exe) -PassThru
& (Join-Path $PSScriptRoot 'front.ps1') -ProcessId $p.Id
"designer pid $($p.Id), started $($p.StartTime)"
