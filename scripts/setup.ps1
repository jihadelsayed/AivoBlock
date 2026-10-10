# AivoBlock local development setup (Windows PowerShell).
# Run from the repository root: .\scripts\setup.ps1
# Checks prerequisites and installs the Python AI engine in a local venv.
# Does not start nodes, change global execution policy, or install system tools.

[CmdletBinding()]
param(
    [switch]$SkipPythonInstall
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$aiDir = Join-Path $root "ai-engine"
$venvDir = Join-Path $aiDir ".venv"

function Find-Command {
    param([string]$Name)
    $command = Get-Command $Name -ErrorAction SilentlyContinue
    if ($null -eq $command) {
        Write-Warning "$Name is not installed or not available on PATH."
        return $false
    }
    Write-Host "[OK] $Name" -ForegroundColor Green
    return $true
}

Write-Host "AivoBlock development environment" -ForegroundColor Cyan
Write-Host "Repository: $root"

$hasPython = Find-Command "python"
$hasCargo = Find-Command "cargo"
$hasCabal = Find-Command "cabal"
$hasGhc = Find-Command "ghc"
$hasDocker = Find-Command "docker"

if ($hasPython -and -not $SkipPythonInstall) {
    $python = Join-Path $venvDir "Scripts\python.exe"

    if (-not (Test-Path $python)) {
        Write-Host "Creating Python virtual environment..."
        & python -m venv $venvDir
        if ($LASTEXITCODE -ne 0) {
            throw "Failed to create Python virtual environment."
        }
    }

    Write-Host "Installing AI engine and development dependencies..."
    & $python -m pip install --editable "$($aiDir)[dev]"
    if ($LASTEXITCODE -ne 0) {
        throw "Python package installation failed."
    }

    Write-Host "Running Python tests..."
    & $python -m pytest (Join-Path $aiDir "tests")
    if ($LASTEXITCODE -ne 0) {
        throw "Python tests failed."
    }
}

Write-Host ""
Write-Host "Prerequisite summary:" -ForegroundColor Cyan
Write-Host "  Python: $hasPython"
Write-Host "  Rust/Cargo: $hasCargo"
Write-Host "  Haskell/Cabal: $hasCabal"
Write-Host "  Haskell/GHC: $hasGhc"
Write-Host "  Docker: $hasDocker"
Write-Host "Setup checks completed. Docker nodes are not started."
