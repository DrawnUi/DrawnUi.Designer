# Builds the designer editor for an iframe in DrawFiddle (the hub inside, see docs/designer.md,
# "Hosting the editor in a page") into web/fiddle/: editor.html, the wasm and its js, drawnui_host.js,
# designer.js and assets/. Every path in it is relative: copy the folder as is (the Fiddle serves it
# from wwwroot/designer/ and loads designer/editor.html).
# Needs emsdk, as vscode/scripts/build-media.ps1 (which it runs). -NoBuild copies the last build.
param([switch]$NoBuild)
$ErrorActionPreference = "Stop"

$root = Resolve-Path "$PSScriptRoot\.."
$out = Join-Path $root "web\fiddle"
if (Test-Path $out) { Remove-Item $out -Recurse -Force }
& "$root\vscode\scripts\build-media.ps1" -Out $out -NoBuild:$NoBuild
if ($LASTEXITCODE) { exit $LASTEXITCODE }
Copy-Item "$root\crates\designer-desktop\web\editor.html" $out
