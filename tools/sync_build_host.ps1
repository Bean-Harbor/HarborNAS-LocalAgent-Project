[CmdletBinding()]
param(
    [string]$TargetRegistryPath = (Join-Path $env:USERPROFILE ".codex\skills\harbor-target-registry\references\targets.md"),
    [Alias("Host")]
    [string]$BuildHost,
    [string]$Username,
    [string]$Password,
    [string]$HarborGatePath,
    [int]$KeepBackupCount = 2
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

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

function Assert-CommandExists {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Name
    )

    $command = Get-Command $Name -ErrorAction SilentlyContinue
    if (-not $command) {
        throw "Required command not found: $Name"
    }

    return $command
}

function Import-PoshSshModule {
    $availableModule = Get-Module -ListAvailable Posh-SSH |
        Sort-Object Version -Descending |
        Select-Object -First 1
    if ($availableModule) {
        Import-Module $availableModule.Path -ErrorAction Stop | Out-Null
        return
    }

    $candidateRoots = @(
        (Join-Path $HOME "Documents\PowerShell\Modules\Posh-SSH"),
        (Join-Path $HOME "Documents\WindowsPowerShell\Modules\Posh-SSH")
    )

    foreach ($root in $candidateRoots) {
        if (-not (Test-Path -LiteralPath $root)) {
            continue
        }

        $manifest = Get-ChildItem -LiteralPath $root -Filter "Posh-SSH.psd1" -Recurse |
            Sort-Object FullName -Descending |
            Select-Object -First 1
        if ($manifest) {
            Import-Module $manifest.FullName -ErrorAction Stop | Out-Null
            return
        }
    }

    throw "Posh-SSH module not found. Install Posh-SSH or make it available to powershell.exe."
}

function Invoke-GitText {
    param(
        [Parameter(Mandatory = $true)]
        [string]$RepoPath,

        [Parameter(Mandatory = $true)]
        [string[]]$Arguments
    )

    $git = Assert-CommandExists -Name "git"
    $output = & $git.Source -C $RepoPath @Arguments 2>&1
    if ($LASTEXITCODE -ne 0) {
        throw "git $($Arguments -join ' ') failed in $RepoPath`n$($output -join [Environment]::NewLine)"
    }

    return $output
}

function Convert-ToShellLiteral {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Value
    )

    return "'" + $Value.Replace("'", "'""'""'") + "'"
}

function Get-BuildHostConfigFromRegistry {
    param(
        [Parameter(Mandatory = $true)]
        [string]$PathValue
    )

    Assert-PathExists -PathValue $PathValue -Label "Target registry"
    $content = Get-Content -LiteralPath $PathValue -Raw
    $match = [regex]::Match(
        $content,
        "(?ms)^\s*-\s*target_id:\s*build-host\s*\r?\n(?<body>.*?)(?=^\s*-\s*target_id:\s|\z)"
    )
    if (-not $match.Success) {
        throw "build-host entry not found in target registry: $PathValue"
    }

    $body = $match.Groups["body"].Value
    $config = [ordered]@{}
    foreach ($key in @("host", "username", "password")) {
        $fieldMatch = [regex]::Match($body, "(?m)^\s*${key}:\s*(?<value>.+?)\s*$")
        if ($fieldMatch.Success) {
            $value = $fieldMatch.Groups["value"].Value.Trim()
            if (($value.StartsWith('"') -and $value.EndsWith('"')) -or ($value.StartsWith("'") -and $value.EndsWith("'"))) {
                $value = $value.Substring(1, $value.Length - 2)
            }
            $config[$key] = $value
        }
    }

    return $config
}

function Resolve-BuildHostConfig {
    param(
        [Parameter(Mandatory = $true)]
        [string]$PathValue,

        [string]$ExplicitHost,
        [string]$ExplicitUsername,
        [string]$ExplicitPassword
    )

    $config = Get-BuildHostConfigFromRegistry -PathValue $PathValue
    if ($ExplicitHost) {
        $config["host"] = $ExplicitHost
    }
    if ($ExplicitUsername) {
        $config["username"] = $ExplicitUsername
    }
    if ($ExplicitPassword) {
        $config["password"] = $ExplicitPassword
    }

    foreach ($key in @("host", "username", "password")) {
        if (-not $config.Contains($key) -or [string]::IsNullOrWhiteSpace([string]$config[$key])) {
            throw "Missing build-host field '$key'. Provide -$($key.Substring(0,1).ToUpper() + $key.Substring(1)) or fix the target registry."
        }
    }

    return $config
}

function Get-SnapshotFileList {
    param(
        [Parameter(Mandatory = $true)]
        [string]$RepoPath
    )

    $lines = Invoke-GitText -RepoPath $RepoPath -Arguments @("ls-files", "--cached", "--others", "--exclude-standard")
    $relativeFiles = New-Object System.Collections.Generic.List[string]

    foreach ($line in $lines) {
        $relativePath = [string]$line
        if ([string]::IsNullOrWhiteSpace($relativePath)) {
            continue
        }

        $relativePath = $relativePath.Trim()
        $absolutePath = Join-Path $RepoPath $relativePath
        if (-not (Test-Path -LiteralPath $absolutePath -PathType Leaf)) {
            continue
        }

        $relativeFiles.Add($relativePath.Replace("\", "/"))
    }

    $unique = $relativeFiles | Sort-Object -Unique
    if (-not $unique -or $unique.Count -eq 0) {
        throw "No files selected for snapshot in $RepoPath"
    }

    return $unique
}

function New-SnapshotArchive {
    param(
        [Parameter(Mandatory = $true)]
        [string]$RepoName,

        [Parameter(Mandatory = $true)]
        [string]$RepoPath,

        [Parameter(Mandatory = $true)]
        [string]$WorkingDirectory,

        [Parameter(Mandatory = $true)]
        [string]$Timestamp
    )

    $tar = Assert-CommandExists -Name "tar"
    $fileList = Get-SnapshotFileList -RepoPath $RepoPath
    $listPath = Join-Path $WorkingDirectory "$RepoName-files.txt"
    $archivePath = Join-Path $WorkingDirectory "$RepoName-$Timestamp.tar.gz"

    Set-Content -LiteralPath $listPath -Value $fileList -Encoding Ascii
    $output = & $tar.Source -czf $archivePath -C $RepoPath -T $listPath 2>&1
    if ($LASTEXITCODE -ne 0) {
        throw "tar failed while archiving $RepoName`n$($output -join [Environment]::NewLine)"
    }

    if (-not (Test-Path -LiteralPath $archivePath -PathType Leaf)) {
        throw "Archive not created for ${RepoName}: $archivePath"
    }

    return [ordered]@{
        RepoName = $RepoName
        RepoPath = $RepoPath
        FileListPath = $listPath
        ArchivePath = $archivePath
        Timestamp = $Timestamp
    }
}

function Get-RemoteSummaryValue {
    param(
        [Parameter(Mandatory = $true)]
        [string[]]$Lines,

        [Parameter(Mandatory = $true)]
        [string]$Key
    )

    foreach ($line in $Lines) {
        $text = [string]$line
        if ($text.StartsWith("$Key=")) {
            return $text.Substring($Key.Length + 1)
        }
    }

    return $null
}

function Join-DisplayList {
    param(
        [Parameter(Mandatory = $false)]
        [string]$Value
    )

    if ([string]::IsNullOrWhiteSpace($Value) -or $Value -eq "none") {
        return "none"
    }

    return (($Value -split "\|") -join ", ")
}

if ($KeepBackupCount -lt 1) {
    throw "KeepBackupCount must be at least 1."
}

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$defaultHarborGatePath = Join-Path (Split-Path -Parent $repoRoot) "HarborGate"
$resolvedHarborGatePath = if ($HarborGatePath) { $HarborGatePath } else { $defaultHarborGatePath }

Assert-PathExists -PathValue $repoRoot -Label "HarborBeacon repo root"
Assert-PathExists -PathValue $resolvedHarborGatePath -Label "HarborGate repo root"
Assert-PathExists -PathValue (Join-Path $repoRoot ".git") -Label "HarborBeacon git metadata"
Assert-PathExists -PathValue (Join-Path $resolvedHarborGatePath ".git") -Label "HarborGate git metadata"
Assert-CommandExists -Name "git" | Out-Null
Assert-CommandExists -Name "tar" | Out-Null
Import-PoshSshModule

$buildHostConfig = Resolve-BuildHostConfig `
    -PathValue $TargetRegistryPath `
    -ExplicitHost $BuildHost `
    -ExplicitUsername $Username `
    -ExplicitPassword $Password

$timestamp = Get-Date -Format "yyyyMMdd-HHmmss"
$workingDirectory = Join-Path ([System.IO.Path]::GetTempPath()) "harbor-build-sync-$timestamp"
$remoteUploadDir = "/home/$($buildHostConfig.username)/uploads"

$sshSession = $null
$sftpSession = $null

try {
    $null = New-Item -ItemType Directory -Force -Path $workingDirectory

    $harborBeaconArchive = New-SnapshotArchive -RepoName "HarborBeacon" -RepoPath $repoRoot -WorkingDirectory $workingDirectory -Timestamp $timestamp
    $harborGateArchive = New-SnapshotArchive -RepoName "HarborGate" -RepoPath $resolvedHarborGatePath -WorkingDirectory $workingDirectory -Timestamp $timestamp

    $securePassword = ConvertTo-SecureString $buildHostConfig.password -AsPlainText -Force
    $credential = [pscredential]::new($buildHostConfig.username, $securePassword)

    $sshSession = New-SSHSession -ComputerName $buildHostConfig.host -Credential $credential -AcceptKey -ConnectionTimeout 20
    $sftpSession = New-SFTPSession -ComputerName $buildHostConfig.host -Credential $credential -AcceptKey -ConnectionTimeout 20

    $prepareRemote = Invoke-SSHCommand -SessionId $sshSession.SessionId -Command "mkdir -p $(Convert-ToShellLiteral $remoteUploadDir) `"$HOME/src/_incoming`" `"$HOME/src/_backups`""
    if ($prepareRemote.ExitStatus -ne 0) {
        throw "Failed to prepare remote directories on $($buildHostConfig.host)`n$($prepareRemote.Error)"
    }

    Set-SFTPItem -SessionId $sftpSession.SessionId -Path $harborBeaconArchive.ArchivePath -Destination $remoteUploadDir -Force | Out-Null
    Set-SFTPItem -SessionId $sftpSession.SessionId -Path $harborGateArchive.ArchivePath -Destination $remoteUploadDir -Force | Out-Null

    $remoteScript = @'
set -euo pipefail

timestamp=__TIMESTAMP__
keep_backup_count=__KEEP_BACKUPS__
harbor_beacon_archive=__HB_ARCHIVE__
harbor_gate_archive=__HG_ARCHIVE__
src_root="$HOME/src"
incoming_root="$src_root/_incoming"
backup_root="$src_root/_backups"

mkdir -p "$incoming_root" "$backup_root"

prune_backups() {
  local repo_name="$1"
  local keep_count="$2"
  local index=0
  local kept=()

  while IFS= read -r entry; do
    if [[ -z "$entry" ]]; then
      continue
    fi
    index=$((index + 1))
    if [[ "$index" -le "$keep_count" ]]; then
      kept+=("$entry")
    else
      rm -rf "$backup_root/$entry"
    fi
  done < <(find "$backup_root" -mindepth 1 -maxdepth 1 -type d -name "${repo_name}-*" -printf '%f\n' | sort -r)

  if [[ "${#kept[@]}" -eq 0 ]]; then
    echo "none"
  else
    local joined=""
    for entry in "${kept[@]}"; do
      if [[ -n "$joined" ]]; then
        joined="${joined}|${entry}"
      else
        joined="$entry"
      fi
    done
    echo "$joined"
  fi
}

activate_repo() {
  local repo_name="$1"
  local archive_path="$2"
  local required_file="$3"
  local active_dir="$src_root/$repo_name"
  local incoming_dir="$incoming_root/${repo_name}-${timestamp}"
  local backup_dir="$backup_root/${repo_name}-${timestamp}"
  local had_active=0

  rm -rf "$incoming_dir"
  mkdir -p "$incoming_dir"
  tar -xzf "$archive_path" -C "$incoming_dir"

  if [[ ! -f "$incoming_dir/$required_file" ]]; then
    echo "Required file missing after extract: $incoming_dir/$required_file" >&2
    exit 1
  fi

  if [[ -d "$active_dir" ]]; then
    mv "$active_dir" "$backup_dir"
    had_active=1
  fi

  if ! mv "$incoming_dir" "$active_dir"; then
    if [[ "$had_active" -eq 1 && -d "$backup_dir" ]]; then
      mv "$backup_dir" "$active_dir"
    fi
    echo "Failed to activate $repo_name from $incoming_dir" >&2
    exit 1
  fi

  if [[ "$repo_name" == "HarborBeacon" ]]; then
    if [[ -d "$active_dir/tools" ]]; then
      find "$active_dir/tools" -type f -name '*.sh' -exec chmod 755 {} +
    fi
    if [[ -d "$active_dir/scripts" ]]; then
      find "$active_dir/scripts" -type f -name '*.sh' -exec chmod 755 {} +
    fi
  fi

  if [[ "$repo_name" == "HarborGate" && -d "$active_dir/tools" ]]; then
    find "$active_dir/tools" -type f -name '*.sh' -exec chmod 755 {} +
  fi

  if [[ ! -f "$active_dir/$required_file" ]]; then
    echo "Required file missing after activation: $active_dir/$required_file" >&2
    exit 1
  fi

  rm -f "$archive_path"
  local kept
  kept="$(prune_backups "$repo_name" "$keep_backup_count")"
  local repo_key
  repo_key="$(printf '%s' "$repo_name" | tr '[:lower:]' '[:upper:]')"
  echo "SYNC_ACTIVE_${repo_key}=$active_dir"
  echo "SYNC_KEPT_${repo_key}=$kept"
}

activate_repo "HarborBeacon" "$harbor_beacon_archive" "tools/bootstrap_release_builder.sh"
activate_repo "HarborGate" "$harbor_gate_archive" "pyproject.toml"

if [[ ! -f "$src_root/HarborBeacon/tools/build_release_bundle.sh" ]]; then
  echo "Required file missing after activation: $src_root/HarborBeacon/tools/build_release_bundle.sh" >&2
  exit 1
fi

echo "SYNC_TIMESTAMP_HARBORBEACON=$timestamp"
echo "SYNC_TIMESTAMP_HARBORGATE=$timestamp"
echo "SYNC_CHECK_BOOTSTRAP=present"
echo "SYNC_CHECK_BUILD_RELEASE=present"
echo "SYNC_CHECK_HARBORGATE_PYPROJECT=present"
'@

    $remoteScript = $remoteScript.Replace("__TIMESTAMP__", (Convert-ToShellLiteral $timestamp))
    $remoteScript = $remoteScript.Replace("__KEEP_BACKUPS__", $KeepBackupCount.ToString())
    $remoteScript = $remoteScript.Replace("__HB_ARCHIVE__", (Convert-ToShellLiteral "$remoteUploadDir/$([IO.Path]::GetFileName($harborBeaconArchive.ArchivePath))"))
    $remoteScript = $remoteScript.Replace("__HG_ARCHIVE__", (Convert-ToShellLiteral "$remoteUploadDir/$([IO.Path]::GetFileName($harborGateArchive.ArchivePath))"))

    $remoteResult = Invoke-SSHCommand -SessionId $sshSession.SessionId -Command $remoteScript -TimeOut 1800
    if ($remoteResult.ExitStatus -ne 0) {
        $stderr = if ($remoteResult.Error) { $remoteResult.Error } else { $remoteResult.Output }
        throw "Remote sync failed on $($buildHostConfig.host)`n$($stderr -join [Environment]::NewLine)"
    }

    $summaryLines = @($remoteResult.Output | ForEach-Object { [string]$_ })
    foreach ($requiredKey in @("SYNC_ACTIVE_HARBORBEACON", "SYNC_ACTIVE_HARBORGATE", "SYNC_CHECK_BOOTSTRAP", "SYNC_CHECK_BUILD_RELEASE", "SYNC_CHECK_HARBORGATE_PYPROJECT")) {
        if (-not (Get-RemoteSummaryValue -Lines $summaryLines -Key $requiredKey)) {
            throw "Remote sync summary missing key: $requiredKey"
        }
    }

    if ((Get-RemoteSummaryValue -Lines $summaryLines -Key "SYNC_CHECK_BOOTSTRAP") -ne "present") {
        throw "HarborBeacon bootstrap_release_builder.sh missing after sync."
    }
    if ((Get-RemoteSummaryValue -Lines $summaryLines -Key "SYNC_CHECK_BUILD_RELEASE") -ne "present") {
        throw "HarborBeacon build_release_bundle.sh missing after sync."
    }
    if ((Get-RemoteSummaryValue -Lines $summaryLines -Key "SYNC_CHECK_HARBORGATE_PYPROJECT") -ne "present") {
        throw "HarborGate pyproject.toml missing after sync."
    }

    Write-Host ""
    Write-Host "Build host sync completed." -ForegroundColor Green
    Write-Host "Build host              : $($buildHostConfig.host)"
    Write-Host "HarborBeacon timestamp  : $(Get-RemoteSummaryValue -Lines $summaryLines -Key 'SYNC_TIMESTAMP_HARBORBEACON')"
    Write-Host "HarborGate timestamp    : $(Get-RemoteSummaryValue -Lines $summaryLines -Key 'SYNC_TIMESTAMP_HARBORGATE')"
    Write-Host "HarborBeacon active dir : $(Get-RemoteSummaryValue -Lines $summaryLines -Key 'SYNC_ACTIVE_HARBORBEACON')"
    Write-Host "HarborGate active dir   : $(Get-RemoteSummaryValue -Lines $summaryLines -Key 'SYNC_ACTIVE_HARBORGATE')"
    Write-Host "HarborBeacon backups    : $(Join-DisplayList -Value (Get-RemoteSummaryValue -Lines $summaryLines -Key 'SYNC_KEPT_HARBORBEACON'))"
    Write-Host "HarborGate backups      : $(Join-DisplayList -Value (Get-RemoteSummaryValue -Lines $summaryLines -Key 'SYNC_KEPT_HARBORGATE'))"
}
finally {
    if ($sftpSession) {
        Remove-SFTPSession -SessionId $sftpSession.SessionId | Out-Null
    }
    if ($sshSession) {
        Remove-SSHSession -SessionId $sshSession.SessionId | Out-Null
    }
    if ($workingDirectory -and (Test-Path -LiteralPath $workingDirectory)) {
        Remove-Item -LiteralPath $workingDirectory -Recurse -Force
    }
}
