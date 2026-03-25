param(
    [Parameter(Mandatory = $true)]
    [string]$AppId,

    [Parameter(Mandatory = $true)]
    [string]$AppSecret,

    [string]$Domain = "https://open.feishu.cn",
    [string]$Workspace = "",
    [string]$TargetDir = "target_nlu_main",
    [switch]$SkipBuild,
    [switch]$DryRun
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$desktopRoot = Join-Path $repoRoot "harborbeacon-desktop"

if ([string]::IsNullOrWhiteSpace($Workspace)) {
    $Workspace = $repoRoot
}

if (-not (Test-Path (Join-Path $desktopRoot "Cargo.toml"))) {
    throw "Desktop workspace not found: $desktopRoot"
}

Write-Host "Desktop root : $desktopRoot"
Write-Host "Workspace    : $Workspace"
Write-Host "Domain       : $Domain"
Write-Host "Target dir   : $TargetDir"

# 1) Kill old agent processes so Feishu messages are consumed by only one process.
$old = Get-Process -Name "harborbeacon-desktop-app" -ErrorAction SilentlyContinue
if ($old) {
    Write-Host "Found old processes: $($old.Count)"
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

Write-Host "Started harborbeacon-desktop-app PID=$($proc.Id)"
Write-Host "stdout log: $stdoutPath"
Write-Host "stderr log: $stderrPath"
