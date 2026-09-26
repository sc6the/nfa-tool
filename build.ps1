$ErrorActionPreference = "Stop"
$proj = $PSScriptRoot
$previousFlags = $env:CARGO_ENCODED_RUSTFLAGS
$cargoDir = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE ".cargo" }
$rustupDir = if ($env:RUSTUP_HOME) { $env:RUSTUP_HOME } else { Join-Path $env:USERPROFILE ".rustup" }
$env:CARGO_ENCODED_RUSTFLAGS = @(
    "-Ctarget-feature=+crt-static",
    "--remap-path-prefix=$cargoDir=cargo",
    "--remap-path-prefix=$rustupDir=rustup",
    "--remap-path-prefix=$($env:USERPROFILE)=user",
    "--remap-path-prefix=$proj=nfa-tool"
) -join [char]0x1f
Push-Location $proj
try {
    cargo build --release --locked
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
    Copy-Item -LiteralPath (Join-Path $proj "target\release\nfa.exe") -Destination (Join-Path $proj "nfa.exe") -Force
    Get-FileHash -LiteralPath (Join-Path $proj "nfa.exe") -Algorithm SHA256
} finally {
    Pop-Location
    $env:CARGO_ENCODED_RUSTFLAGS = $previousFlags
}
