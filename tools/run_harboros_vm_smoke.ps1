[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$WebSocketUrl,

    [Parameter(Mandatory = $true)]
    [string]$Username,

    [Parameter(Mandatory = $true)]
    [string]$Password,

    [string]$EnvName = "env-a",
    [string]$ProbeService = "ssh",
    [string]$FilesystemPath = "/mnt",
    [string]$ReportDir,
    [switch]$SkipBuild,
    [switch]$RunDrift,
    [string]$DriftHarborRef = "develop",
    [string]$DriftUpstreamRef = "master",
    [string]$HarborRepoPath,
    [string]$UpstreamRepoPath
)

$ErrorActionPreference = "Stop"

function Assert-PathExists {
    param(
        [Parameter(Mandatory = $true)]
        [string]$PathValue,

        [Parameter(Mandatory = $true)]
        [string]$Label
    )

    if (-not (Test-Path -LiteralPath $PathValue)) {
        throw "$Label not found: $PathValue"
    }
}

function Invoke-Step {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Title,

        [Parameter(Mandatory = $true)]
        [scriptblock]$Action
    )

    Write-Host ""
    Write-Host "==> $Title" -ForegroundColor Cyan
    & $Action
}

if ($WebSocketUrl -notmatch "^wss?://") {
    throw "WebSocketUrl must start with ws:// or wss://"
}

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$cliShim = (Resolve-Path (Join-Path $PSScriptRoot "cli.cmd")).Path
$venvPython = Join-Path $repoRoot ".venv\Scripts\python.exe"
$releaseDir = Join-Path $repoRoot "target\release"
$validateExe = Join-Path $releaseDir "validate-contract-schemas.exe"
$e2eExe = Join-Path $releaseDir "run-e2e-suite.exe"
$driftExe = Join-Path $releaseDir "run-drift-matrix.exe"

Assert-PathExists -PathValue $cliShim -Label "CLI shim"
Assert-PathExists -PathValue $venvPython -Label "Python venv"

if (-not $ReportDir) {
    $ReportDir = Join-Path $repoRoot ".tmp-live\harboros-vm-smoke"
}

$null = New-Item -ItemType Directory -Force -Path $ReportDir
$timestamp = Get-Date -Format "yyyyMMdd-HHmmss"
$validateReport = Join-Path $ReportDir "validate-contract-$timestamp.json"
$e2eReport = Join-Path $ReportDir "e2e-$timestamp.json"
$driftReport = Join-Path $ReportDir "drift-$timestamp.json"

if (-not $SkipBuild) {
    $missingReleaseBinary = @($validateExe, $e2eExe) | Where-Object { -not (Test-Path -LiteralPath $_) }
    if ($RunDrift -and -not (Test-Path -LiteralPath $driftExe)) {
        $missingReleaseBinary += $driftExe
    }

    if ($missingReleaseBinary.Count -gt 0) {
        $cargo = Get-Command cargo -ErrorAction SilentlyContinue
        if (-not $cargo) {
            throw "cargo not found and required release binaries are missing. Install Rust or rerun with -SkipBuild after building."
        }

        Invoke-Step -Title "Building release binaries" -Action {
            $cargoArgs = @("build", "--release", "--bin", "validate-contract-schemas", "--bin", "run-e2e-suite")
            if ($RunDrift) {
                $cargoArgs += @("--bin", "run-drift-matrix")
            }

            Push-Location $repoRoot
            try {
                & $cargo.Source @cargoArgs
                if ($LASTEXITCODE -ne 0) {
                    throw "cargo build for HarborOS smoke binaries failed"
                }
            }
            finally {
                Pop-Location
            }
        }
    }
}

Assert-PathExists -PathValue $validateExe -Label "validate-contract-schemas binary"
Assert-PathExists -PathValue $e2eExe -Label "run-e2e-suite binary"

if ($RunDrift) {
    Assert-PathExists -PathValue $driftExe -Label "run-drift-matrix binary"
}

$env:HARBOR_MIDCLI_BIN = $cliShim
$env:HARBOR_MIDCLI_URL = $WebSocketUrl
$env:HARBOR_MIDCLI_USER = $Username
$env:HARBOR_MIDCLI_PASSWORD = $Password
$env:HARBOR_PROBE_SERVICE = $ProbeService
$env:HARBOR_FILESYSTEM_PATH = $FilesystemPath

if ($HarborRepoPath) {
    $env:HARBOR_SOURCE_REPO_PATH = $HarborRepoPath
}

if ($UpstreamRepoPath) {
    $env:UPSTREAM_SOURCE_REPO_PATH = $UpstreamRepoPath
}

Write-Host "Repo root      : $repoRoot"
Write-Host "WebSocket URL  : $WebSocketUrl"
Write-Host "Probe service  : $ProbeService"
Write-Host "Filesystem path: $FilesystemPath"
Write-Host "Report dir     : $ReportDir"
Write-Host "Run drift      : $($RunDrift.IsPresent)"

Invoke-Step -Title "Running validate-contract-schemas live probe" -Action {
    Push-Location $repoRoot
    try {
        & $validateExe --require-live --report $validateReport
        if ($LASTEXITCODE -ne 0) {
            throw "validate-contract-schemas live probe failed"
        }
    }
    finally {
        Pop-Location
    }
}

Invoke-Step -Title "Running run-e2e-suite live probe" -Action {
    Push-Location $repoRoot
    try {
        & $e2eExe --env $EnvName --require-live --report $e2eReport
        if ($LASTEXITCODE -ne 0) {
            throw "run-e2e-suite live probe failed"
        }
    }
    finally {
        Pop-Location
    }
}

if ($RunDrift) {
    Invoke-Step -Title "Running run-drift-matrix" -Action {
        $args = @(
            "--harbor-ref", $DriftHarborRef,
            "--upstream-ref", $DriftUpstreamRef,
            "--report", $driftReport
        )

        if ($HarborRepoPath) {
            $args += @("--harbor-repo-path", $HarborRepoPath)
        }

        if ($UpstreamRepoPath) {
            $args += @("--upstream-repo-path", $UpstreamRepoPath)
        }

        Push-Location $repoRoot
        try {
            & $driftExe @args
            if ($LASTEXITCODE -ne 0) {
                throw "run-drift-matrix failed"
            }
        }
        finally {
            Pop-Location
        }
    }
}

Write-Host ""
Write-Host "Smoke run completed." -ForegroundColor Green
Write-Host "Validate report: $validateReport"
Write-Host "E2E report     : $e2eReport"

if ($RunDrift) {
    Write-Host "Drift report   : $driftReport"
}
