$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path $PSScriptRoot -Parent
Push-Location $projectRoot
try {
    cargo test --locked
    if ($LASTEXITCODE) { throw 'Tests failed' }
    cargo build --locked --release --bin chd312-background
    if ($LASTEXITCODE) { throw 'Background build failed' }
    cargo build --locked --release --bin chd312 --features bundle-worker
    if ($LASTEXITCODE) { throw 'Portable build failed' }
    cargo build --locked --release --manifest-path installer/Cargo.toml
    if ($LASTEXITCODE) { throw 'Installer build failed' }
    $null = New-Item -ItemType Directory -Path 'dist' -Force
    Copy-Item -LiteralPath 'target/release/chd312.exe' -Destination 'dist/chd312-portable-x64.exe' -Force
    Copy-Item -LiteralPath 'installer/target/release/chd312-setup.exe' -Destination 'dist/chd312-setup-x64.exe' -Force
    $checksums = @('chd312-portable-x64.exe','chd312-setup-x64.exe') | ForEach-Object {
        $hash = Get-FileHash -LiteralPath (Join-Path 'dist' $_) -Algorithm SHA256
        $hash.Hash.ToLowerInvariant() + '  ' + $_
    }
    $checksums | Set-Content -LiteralPath 'dist/SHA256SUMS.txt' -Encoding ascii
} finally { Pop-Location }
