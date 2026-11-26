# Local mirror of .github/workflows/ci.yml. Run before calling work done.
#
#   pwsh scripts/ci_local.ps1            full gate (what CI runs)
#   pwsh scripts/ci_local.ps1 -Quick     check + clippy + fmt, both feature sets
#
# Exit code is non-zero on the first failing step; the step name is printed.
param([switch]$Quick)

$ErrorActionPreference = "Stop"
$RepoRoot = Split-Path $PSScriptRoot -Parent
Set-Location $RepoRoot
$env:CARGO_TERM_COLOR = "always"

function Step($Name, [scriptblock]$Body) {
    Write-Host "`n==> $Name" -ForegroundColor Cyan
    & $Body
    if ($LASTEXITCODE -ne 0) { Write-Host "FAILED: $Name" -ForegroundColor Red; exit $LASTEXITCODE }
}

Step "fmt --check" { cargo fmt --all -- --check }
Step "clippy (default)" { cargo clippy --workspace --lib --bins --tests -- -D warnings }
Step "clippy (editor)"  { cargo clippy --workspace --lib --bins --tests --features editor -- -D warnings }

if ($Quick) { Write-Host "`nQuick gate passed." -ForegroundColor Green; exit 0 }

Step "build (default)" { cargo build --workspace }
Step "build (editor)"  { cargo build -p game_client --features editor }
Step "test (default)"  { cargo test --workspace --lib --tests }
Step "test (editor)"   { cargo test --workspace --lib --tests --features editor }

Step "graph crates standalone" { cargo check -p node_graph_types -p node_graph_exec -p curve_asset }
if ((rustup target list --installed) -contains "wasm32-unknown-unknown") {
    Step "wasm32 game_shared"   { cargo check -p game_shared --target wasm32-unknown-unknown }
    Step "wasm32 graph crates"  { cargo check -p node_graph_types -p node_graph_exec -p curve_asset --target wasm32-unknown-unknown }
} else {
    Write-Host "`n(skipping wasm32 checks: run 'rustup target add wasm32-unknown-unknown')" -ForegroundColor Yellow
}

if (Get-Command bash -ErrorAction SilentlyContinue) {
    Step "design lint" { bash scripts/lint_design.sh }
} else {
    Write-Host "`n(skipping design lint: bash not found)" -ForegroundColor Yellow
}

Write-Host "`nFull gate passed." -ForegroundColor Green
