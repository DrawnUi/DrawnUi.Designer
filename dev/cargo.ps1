# cargo for working on this repository together with DrawnUI for Rust: the same arguments as cargo.
# When a DrawnUI for Rust checkout is found (DRAWNUI_RUST_DIR, or a DrawnUi.Rust folder next to this
# repository), drawnui is built from that source; otherwise from crates.io, as for everyone who clones
# this repository. The Cargo.lock files keep the crates.io entry either way.
#   dev\cargo.ps1 test -p designer-desktop
#   $env:DRAWNUI_RUST_DIR = 'C:\src\DrawnUi.Rust'; dev\cargo.ps1 run -p designer-desktop --release
$repo = Split-Path $PSScriptRoot -Parent
$engine = if ($env:DRAWNUI_RUST_DIR) { $env:DRAWNUI_RUST_DIR } else { Join-Path (Split-Path $repo -Parent) 'DrawnUi.Rust' }
$crate = Join-Path $engine 'drawnui\Cargo.toml'
if (-not (Test-Path $crate)) {
    & cargo @args
    exit $LASTEXITCODE
}
$source = (Split-Path (Resolve-Path $crate).Path -Parent) -replace '\\', '/'
Write-Host "drawnui from source: $source"
$locks = @("$repo\Cargo.lock", "$repo\agents\rust\Cargo.lock") | Where-Object { Test-Path $_ }
$saved = @{}
foreach ($lock in $locks) { $saved[$lock] = [IO.File]::ReadAllBytes($lock) }
try {
    & cargo --config "patch.crates-io.drawnui.path='$source'" @args
    $code = $LASTEXITCODE
} finally {
    foreach ($lock in $locks) { [IO.File]::WriteAllBytes($lock, $saved[$lock]) }
}
exit $code
