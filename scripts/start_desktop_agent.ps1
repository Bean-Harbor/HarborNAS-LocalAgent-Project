
param(
    [Parameter(Mandatory = $true)]
    [string]$AppId,

    [Parameter(Mandatory = $true)]
    [string]$AppSecret,

    [string]$Domain = "https://open.feishu.cn",
    [string]$Workspace = "",
    [string]$TargetDir = "",
    [int]$HealthTimeoutSec = 8,
    [switch]$NoHealthCheck,
    [switch]$SkipBuild,
    [switch]$DryRun
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$desktopRoot = Join-Path $repoRoot "harborbeacon-desktop"

function Resolve-PreferredTargetDir {
    param(
        [string]$DesktopRoot,
        [string]$RequestedTargetDir,
        [bool]$SkipBuild
    )

    # Priority:
    # 1) Explicit parameter
    # 2) Environment override
    # 3) Latest existing target_nlu_fix* executable
    # 4) target_nlu_main
    if (-not [string]::IsNullOrWhiteSpace($RequestedTargetDir)) {
        return $RequestedTargetDir
    }

    $envTarget = $env:HARBOR_DESKTOP_TARGET_DIR
    if (-not [string]::IsNullOrWhiteSpace($envTarget)) {
        return $envTarget
    }

    $candidateFix = @(Get-ChildItem -Path $DesktopRoot -Directory -Filter "target_nlu_fix*" -ErrorAction SilentlyContinue |
        ForEach-Object {
            $exe = Join-Path $_.FullName "debug\harborbeacon-desktop-app.exe"
            if (Test-Path $exe) {
                [PSCustomObject]@{
                    DirName = $_.Name
                    ExePath = $exe
                    LastWriteTime = (Get-Item $exe).LastWriteTime
                }
            }
        } |
        Where-Object { $_ -ne $null } |
        Sort-Object LastWriteTime -Descending)

    if ($candidateFix.Length -gt 0) {
        return $candidateFix[0].DirName
    }

    if ($SkipBuild) {
        return "target_nlu_main"
    }

    return "target_nlu_main"
}

function Get-ExeFingerprint {
    param([string]$ExePath)

    if (-not (Test-Path $ExePath)) {
        return "<missing>"
    }

    $item = Get-Item $ExePath
    $hash = (Get-FileHash -Path $ExePath -Algorithm SHA256).Hash
    $shortHash = $hash.Substring(0, 12)
    $utc = $item.LastWriteTimeUtc.ToString("yyyy-MM-dd HH:mm:ss 'UTC'")
    return "mtime=$utc, size=$($item.Length), sha256=$shortHash"
}

if ([string]::IsNullOrWhiteSpace($Workspace)) {
    $Workspace = $repoRoot
}

if (-not (Test-Path (Join-Path $desktopRoot "Cargo.toml"))) {
    throw "Desktop workspace not found: $desktopRoot"
}

$TargetDir = Resolve-PreferredTargetDir -DesktopRoot $desktopRoot -RequestedTargetDir $TargetDir -SkipBuild:$SkipBuild

Write-Host "Desktop root : $desktopRoot"
Write-Host "Workspace    : $Workspace"
Write-Host "Domain       : $Domain"
Write-Host "Target dir   : $TargetDir"

# 1) Kill old agent processes so Feishu messages are consumed by only one process.
$old = @(Get-Process -Name "harborbeacon-desktop-app" -ErrorAction SilentlyContinue)
if ($old.Length -gt 0) {
    Write-Host "Found old processes: $($old.Length)"
    $old | Select-Object Id, StartTime | Format-Table -AutoSize
    if (-not $DryRun) {
        $old | Stop-Process
        Write-Host "Stopped old harborbeacon-desktop-app processes."
    } else {
        Write-Host "DryRun: skip stopping old processes."
    }
} else {
    Write-Host "No old harborbeacon-desktop-app process found."
}

# 2) Build (optional) and locate executable.
$exePath = Join-Path $desktopRoot "$TargetDir\debug\harborbeacon-desktop-app.exe"
if (-not (Test-Path $exePath) -and $SkipBuild) {
    throw "Executable not found and -SkipBuild specified: $exePath"
}

if (-not $SkipBuild) {
    if ($DryRun) {
        Write-Host "DryRun: skip cargo build"
    } else {
        Push-Location $desktopRoot
        try {
            $env:CARGO_TARGET_DIR = $TargetDir
            cargo build --target-dir $TargetDir -p harborbeacon-desktop-app
        } finally {
            Pop-Location
        }
    }
}

if (-not (Test-Path $exePath)) {
    throw "Executable not found after build: $exePath"
}

$exeFingerprint = Get-ExeFingerprint -ExePath $exePath
Write-Host "Build fingerprint: $exeFingerprint"

# 3) Start one new process and redirect logs.
$logDir = Join-Path $desktopRoot "runlogs"
if (-not (Test-Path $logDir) -and -not $DryRun) {
    New-Item -ItemType Directory -Path $logDir | Out-Null
}

$stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$stdoutPath = Join-Path $logDir "desktop-agent-$stamp.log"
$stderrPath = Join-Path $logDir "desktop-agent-$stamp.err.log"

$args = @(
    "--app-id", $AppId,
    "--app-secret", $AppSecret,
    "--domain", $Domain,
    "--workspace", $Workspace
)

Write-Host "Command:"
Write-Host "$exePath $($args -join ' ')"

if ($DryRun) {
    Write-Host "DryRun: skip process start"
    exit 0
}

$proc = Start-Process -FilePath $exePath -WorkingDirectory $desktopRoot -ArgumentList $args -RedirectStandardOutput $stdoutPath -RedirectStandardError $stderrPath -PassThru

Start-Sleep -Seconds 1

$alive = @(Get-Process -Id $proc.Id -ErrorAction SilentlyContinue)
if ($alive.Length -eq 0) {
    Write-Host "Process exited immediately. Recent stderr:" -ForegroundColor Yellow
    if (Test-Path $stderrPath) {
        Get-Content $stderrPath -ErrorAction SilentlyContinue | Select-Object -Last 40
    }
    throw "harborbeacon-desktop-app exited immediately after start."
}

Write-Host "Started harborbeacon-desktop-app PID=$($proc.Id)"
Write-Host "stdout log: $stdoutPath"
Write-Host "stderr log: $stderrPath"
Write-Host "Running build   : $TargetDir ($exeFingerprint)"

$dup = @(Get-Process -Name "harborbeacon-desktop-app" -ErrorAction SilentlyContinue)
if ($dup.Length -gt 1) {
    Write-Host "Warning: multiple harborbeacon-desktop-app processes detected:" -ForegroundColor Yellow
    $dup | Select-Object Id, StartTime | Format-Table -AutoSize
}

if (-not $NoHealthCheck) {
    $timeout = [Math]::Max(2, $HealthTimeoutSec)
    $ok = $false
    for ($i = 0; $i -lt $timeout; $i++) {
        Start-Sleep -Seconds 1
        $aliveNow = @(Get-Process -Id $proc.Id -ErrorAction SilentlyContinue)
        if ($aliveNow.Length -eq 0) {
            Write-Host "HealthCheck: process exited during startup window." -ForegroundColor Yellow
            if (Test-Path $stderrPath) {
                Get-Content $stderrPath -ErrorAction SilentlyContinue | Select-Object -Last 40
            }
            throw "Startup health check failed: process exited."
        }
        if (Test-Path $stderrPath) {
            $errTail = (Get-Content $stderrPath -ErrorAction SilentlyContinue | Select-Object -Last 20) -join "`n"
            if ($errTail -match "panic|thread 'main' panicked|Failed to start Feishu WS|error:") {
                Write-Host "HealthCheck: detected error patterns in stderr." -ForegroundColor Yellow
                Write-Host $errTail
                throw "Startup health check failed: detected startup errors in stderr."
            }
        }
        $ok = $true
    }
    if ($ok) {
        Write-Host "HealthCheck: process stayed alive for ${timeout}s (PASS)." -ForegroundColor Green
    }
}
