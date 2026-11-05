# Builds the designer's web UI (crates/designer-desktop for wasm32-unknown-emscripten) and copies it,
# with its page scripts and assets, into media/ for the webview.
# Needs emsdk: em++ on PATH, or EMSDK pointing at the emsdk folder (its emsdk_env.ps1 is run here).
# -NoBuild copies the last build. -Out copies into another folder (dev/build-fiddle-editor.ps1).
param([switch]$NoBuild, [string]$Out)
$ErrorActionPreference = "Stop"

$crate = Resolve-Path "$PSScriptRoot\..\..\crates\designer-desktop"
$media = if ($Out) { [IO.Path]::GetFullPath($Out, (Get-Location).Path) } else { Join-Path $PSScriptRoot "..\media" }

Push-Location $crate
try {
    if (-not $NoBuild) {
        if (-not (Get-Command em++ -ErrorAction SilentlyContinue)) {
            if (-not $env:EMSDK) { throw "em++ is not on PATH and EMSDK is not set" }
            # Git Bash's MSYSTEM makes emsdk_env write a Unix-style PATH.
            Remove-Item env:MSYSTEM -ErrorAction SilentlyContinue
            $env:EMSDK_QUIET = "1"
            . (Join-Path $env:EMSDK "emsdk_env.ps1") | Out-Null
        }
        # The C parts (tree-sitter, for the hub of page mode) build with emsdk's own compilers;
        # cc-rs would look for emcc.bat, which emsdk on Windows does not have.
        $empp = (Get-Command em++).Source
        $emscripten, $ext = (Split-Path $empp), [IO.Path]::GetExtension($empp)
        $env:CC_wasm32_unknown_emscripten = Join-Path $emscripten "emcc$ext"
        $env:CXX_wasm32_unknown_emscripten = Join-Path $emscripten "em++$ext"
        $env:AR_wasm32_unknown_emscripten = Join-Path $emscripten "emar$ext"
        # Built from the crate folder: its .cargo/config.toml holds the web link flags.
        cargo build -j 12 --release --target wasm32-unknown-emscripten
        if ($LASTEXITCODE) { exit $LASTEXITCODE }
    }
    # -AsHashtable: crate features may differ only by case ("USB" / "usb"), which a PSObject cannot hold.
    $metadata = cargo metadata --format-version 1 | ConvertFrom-Json -AsHashtable
    $built = Join-Path $metadata.target_directory "wasm32-unknown-emscripten\release"
    $drawnui = $metadata.packages | Where-Object { $_.name -eq "drawnui" }

    New-Item -ItemType Directory -Force $media | Out-Null
    Copy-Item "$built\drawnui-designer.js", "$built\drawnui_designer.wasm" $media -Force
    # The page script of the drawnui version this build linked: it and the wasm speak one protocol.
    Copy-Item (Join-Path (Split-Path $drawnui.manifest_path) "web\drawnui_host.js") $media -Force
    Copy-Item "web\designer.js" $media -Force
    Copy-Item assets $media -Recurse -Force
    Write-Host "copied to $(Resolve-Path $media)"
}
finally {
    Pop-Location
}
