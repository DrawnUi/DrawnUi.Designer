# Packs the VS Code extension once per platform, each .vsix carrying that platform's designer hub (bin/), the
# Marketplace way for extensions with a native program: target\dist\drawnui-designer-<target>-<version>.vsix.
#   win32-x64, win32-arm64: the exes dev\release.ps1 built (target\dist\drawnui-designer-<version>-windows-*.exe).
#   darwin-arm64, darwin-x64: the universal binary inside the notarized app, from the macOS release zip
#     (target\dist\DrawnUI-Designer-<version>-macos-universal.zip, made by dev/release.sh on the Mac).
# The web UI (media/) is built once from DrawnUI for Rust on crates.io (vscode\scripts\build-media.ps1 needs
# emsdk: EMSDK set or em++ on PATH). Packages are pre-release while the designer is a preview.
# Usage: dev\package-vscode.ps1 [-Targets win32-x64,win32-arm64,darwin-arm64,darwin-x64] [-NoMedia] [-Release]
param([string[]]$Targets = @('win32-x64', 'win32-arm64', 'darwin-arm64', 'darwin-x64'), [switch]$NoMedia, [switch]$Release)
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path "$PSScriptRoot\..").Path
$dist = Join-Path $repo 'target\dist'
$vscode = Join-Path $repo 'vscode'
$version = (Get-Content (Join-Path $vscode 'package.json') -Raw | ConvertFrom-Json).version
$bin = Join-Path $vscode 'bin'

function Hub([string]$target) {
    switch ($target) {
        'win32-x64' { return @{ From = Join-Path $dist "drawnui-designer-$version-windows-x64.exe"; Name = 'drawnui-designer.exe' } }
        'win32-arm64' { return @{ From = Join-Path $dist "drawnui-designer-$version-windows-arm64.exe"; Name = 'drawnui-designer.exe' } }
        { $_ -in 'darwin-arm64', 'darwin-x64' } {
            $zip = Join-Path $dist "DrawnUI-Designer-$version-macos-universal.zip"
            if (-not (Test-Path $zip)) { throw "no $zip (dev/release.sh on the Mac, then copy it here)" }
            $unpacked = Join-Path $dist 'mac-unpacked'
            if (-not (Test-Path $unpacked)) { Expand-Archive $zip $unpacked -Force }
            return @{ From = Join-Path $unpacked 'DrawnUI Designer.app\Contents\MacOS\drawnui-designer'; Name = 'drawnui-designer' }
        }
        default { throw "unknown target $target" }
    }
}

Push-Location $vscode
try {
    if (-not $NoMedia) {
        npm run media
        if ($LASTEXITCODE) { throw 'the web UI did not build' }
    }
    foreach ($target in $Targets) {
        $hub = Hub $target
        if (-not (Test-Path $hub.From)) { throw "no hub for ${target}: $($hub.From) (dev\release.ps1 first)" }
        # bin/ holds this platform's hub only
        if (Test-Path $bin) { Get-ChildItem $bin | Remove-Item -Force }
        New-Item -ItemType Directory -Force $bin | Out-Null
        Copy-Item $hub.From (Join-Path $bin $hub.Name) -Force
        $out = Join-Path $dist "drawnui-designer-$target-$version.vsix"
        $flags = @('--target', $target, '--out', $out)
        if (-not $Release) { $flags += '--pre-release' }
        npm run package -- @flags
        if ($LASTEXITCODE) { throw "packing $target failed" }
        "$out"
    }
}
finally {
    Pop-Location
    $unpacked = Join-Path $dist 'mac-unpacked'
    if (Test-Path $unpacked) { Remove-Item -Recurse -Force $unpacked }
}
