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
    [switch]$AllowMutations,
    [string]$MutationRoot = "/mnt/software/harborbeacon-agent-ci",
    [string]$ApprovalToken,
    [string]$RequiredApprovalToken,
    [string]$ApproverId,
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

function Get-LatestWriteTimeUtc {
    param(
        [Parameter(Mandatory = $true)]
        [string]$PathValue
    )

    if (-not (Test-Path -LiteralPath $PathValue)) {
        return [datetime]::MinValue
    }

    $item = Get-Item -LiteralPath $PathValue
    if (-not $item.PSIsContainer) {
        return $item.LastWriteTimeUtc
    }

    $latest = Get-ChildItem -LiteralPath $PathValue -Recurse -File |
        Sort-Object LastWriteTimeUtc -Descending |
        Select-Object -First 1

    if ($latest) {
        return $latest.LastWriteTimeUtc
    }

    return $item.LastWriteTimeUtc
}

function Test-BinaryNeedsBuild {
    param(
        [Parameter(Mandatory = $true)]
        [string]$BinaryPath,

        [Parameter(Mandatory = $true)]
        [string[]]$SourcePaths
    )

    if (-not (Test-Path -LiteralPath $BinaryPath)) {
        return $true
    }

    $binaryWriteTime = (Get-Item -LiteralPath $BinaryPath).LastWriteTimeUtc
    foreach ($sourcePath in $SourcePaths) {
        $sourceWriteTime = Get-LatestWriteTimeUtc -PathValue $sourcePath
        if ($sourceWriteTime -gt $binaryWriteTime) {
            return $true
        }
    }

    return $false
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

function Convert-ToMiddlewareApiUri {
    param(
        [Parameter(Mandatory = $true)]
        [string]$SocketUri
    )

    if ($SocketUri -match "/api/current/?$") {
        return $SocketUri
    }

    if ($SocketUri -match "/websocket/?$") {
        return ($SocketUri -replace "/websocket/?$", "/api/current")
    }

    return ($SocketUri.TrimEnd("/")) + "/api/current"
}

function Resolve-MidcltBinary {
    $command = Get-Command midclt -ErrorAction SilentlyContinue
    if ($command) {
        return $command.Source
    }

    $userScripts = Join-Path $env:APPDATA "Python\Python312\Scripts\midclt.exe"
    if (Test-Path -LiteralPath $userScripts) {
        return $userScripts
    }

    return $null
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
    $buildInputs = @(
        (Join-Path $repoRoot "Cargo.toml"),
        (Join-Path $repoRoot "Cargo.lock"),
        (Join-Path $repoRoot "src")
    )
    $requiredBinaries = @($validateExe, $e2eExe)
    if ($RunDrift) {
        $requiredBinaries += $driftExe
    }

    $missingReleaseBinary = $requiredBinaries | Where-Object { -not (Test-Path -LiteralPath $_) }
    $staleReleaseBinary = $requiredBinaries | Where-Object {
        Test-BinaryNeedsBuild -BinaryPath $_ -SourcePaths $buildInputs
    }

    if ($missingReleaseBinary.Count -gt 0 -or $staleReleaseBinary.Count -gt 0) {
        $cargo = Get-Command cargo -ErrorAction SilentlyContinue
        if (-not $cargo) {
            throw "cargo not found and required release binaries are missing. Install Rust or rerun with -SkipBuild after building."
        }

        Invoke-Step -Title "Building release binaries" -Action {
            if ($missingReleaseBinary.Count -gt 0) {
                Write-Host ("Missing binaries: " + ($missingReleaseBinary -join ", "))
            }
            if ($staleReleaseBinary.Count -gt 0) {
                Write-Host ("Stale binaries: " + ($staleReleaseBinary -join ", "))
            }

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
if ($AllowMutations.IsPresent) {
    $env:HARBOR_ALLOW_MUTATIONS = "1"
} else {
    Remove-Item Env:HARBOR_ALLOW_MUTATIONS -ErrorAction SilentlyContinue
}
$env:HARBOR_MUTATION_ROOT = $MutationRoot
if ($ApprovalToken) {
    $env:HARBOR_APPROVAL_TOKEN = $ApprovalToken
} else {
    Remove-Item Env:HARBOR_APPROVAL_TOKEN -ErrorAction SilentlyContinue
}
if ($RequiredApprovalToken) {
    $env:HARBOR_REQUIRED_APPROVAL_TOKEN = $RequiredApprovalToken
} else {
    Remove-Item Env:HARBOR_REQUIRED_APPROVAL_TOKEN -ErrorAction SilentlyContinue
}
if ($ApproverId) {
    $env:HARBOR_APPROVER_ID = $ApproverId
} else {
    Remove-Item Env:HARBOR_APPROVER_ID -ErrorAction SilentlyContinue
}
if (-not $env:HARBOR_MIDCLI_TIMEOUT) {
    $env:HARBOR_MIDCLI_TIMEOUT = "5000"
}
if (-not $env:HARBOR_MIDDLEWARE_TIMEOUT) {
    $env:HARBOR_MIDDLEWARE_TIMEOUT = "5000"
}

if (-not $env:HARBOR_MIDDLEWARE_BIN) {
    $midcltBinary = Resolve-MidcltBinary
    if ($midcltBinary) {
        $middlewareUri = Convert-ToMiddlewareApiUri -SocketUri $WebSocketUrl
        $middlewareWrapper = Join-Path $ReportDir "midclt-remote.cmd"
        $wrapperContents = @"
@echo off
"$midcltBinary" -u $middlewareUri -U $Username -P $Password %*
"@
        Set-Content -LiteralPath $middlewareWrapper -Value $wrapperContents -Encoding ASCII
        $env:HARBOR_MIDDLEWARE_BIN = $middlewareWrapper
    }
}

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
Write-Host "Mutations      : $($AllowMutations.IsPresent)"
Write-Host "Mutation root  : $MutationRoot"
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
