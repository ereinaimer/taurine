$ErrorActionPreference = "Stop"

$Platform = "windows-x86_64"
$MaxRetries = 3
$RetryDelay = 2

function Show-SpinnerJob ($Job, $Label) {
    $spinstr = @('⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏')
    $i = 0
    while ($Job.State -eq "Running") {
        $frame = $spinstr[$i % $spinstr.Length]
        Write-Host -NoNewline "`r$frame $Label"
        $i++
        Start-Sleep -Milliseconds 80
    }
    Write-Host -NoNewline "`r"
    Write-Host -ForegroundColor Green -NoNewline "✓ "
    Write-Host $Label
    $result = Receive-Job -Job $Job
    Remove-Job -Job $Job
    return $result
}

function Invoke-WithRetry ($ScriptBlock, $ArgumentList, $Label) {
    $attempt = 0
    $delay = $RetryDelay
    while ($attempt -lt $MaxRetries) {
        $job = Start-Job -ScriptBlock $ScriptBlock -ArgumentList $ArgumentList
        $result = Show-SpinnerJob $job $Label
        if ($null -ne $result -and ($result -is [string] -or $result.PSObject.Properties.Name -contains 'version')) {
            return $result
        }
        $attempt++
        if ($attempt -lt $MaxRetries) {
            Write-Host "  Retrying in ${delay}s... ($attempt/$MaxRetries)"
            Start-Sleep -Seconds $delay
            $delay *= 2
        }
    }
    Write-Error "Failed after $MaxRetries attempts: $Label"
    exit 1
}

# Fetch manifest with retry
$ManifestJob = {
    param($url)
    Invoke-RestMethod -Uri $url
}
$Manifest = Invoke-WithRetry -ScriptBlock $ManifestJob -ArgumentList @("https://github.com/ereinaimer/taurine/releases/latest/download/manifest.json") -Label "Fetching latest release manifest"
$Version = $Manifest.version
$Url = $Manifest.artifacts.$Platform.url
$Sha256 = $Manifest.artifacts.$Platform.sha256

if (-not $Version -or -not $Url) {
    Write-Error "Could not determine latest version or download URL."
    exit 1
}

$InstallDir = Join-Path $env:LOCALAPPDATA "Taurine\bin"
$ExePath = Join-Path $InstallDir "taurine.exe"

# Check if already installed — gracefully handle old binaries without --version
$LocalVersion = $null
if (Test-Path $ExePath) {
    try {
        $versionOutput = & $ExePath --version 2>$null
        if ($versionOutput) {
            $LocalVersion = ($versionOutput -split " ")[1]
        }
    } catch {
        # --version flag not supported, treat as unknown version
        Write-Host "  Existing binary does not support --version check — will reinstall"
    }
}

if ($LocalVersion) {
    if ($LocalVersion -eq $Version) {
        Write-Host "Taurine is already installed and up to date (v$LocalVersion)."
        exit 0
    }
    # Prevent downgrade
    if ([version]$LocalVersion -gt [version]$Version) {
        Write-Host "Taurine v$LocalVersion is newer than the latest release (v$Version). Skipping update."
        exit 0
    }
}

$TempZip = Join-Path $env:TEMP "taurine-$([guid]::NewGuid()).zip"

# Download archive with retry
$DownloadJob = {
    param($url, $out)
    Invoke-WebRequest -Uri $url -OutFile $out -UseBasicParsing
    return $out
}
Invoke-WithRetry -ScriptBlock $DownloadJob -ArgumentList @($Url, $TempZip) -Label "Downloading taurine v$Version" | Out-Null

# Verify checksum if available
if ($Sha256) {
    $computedHash = (Get-FileHash -Path $TempZip -Algorithm SHA256).Hash.ToLower()
    $expectedHash = $Sha256.ToLower()
    if ($computedHash -ne $expectedHash) {
        Write-Error "Checksum verification failed for downloaded archive."
        Remove-Item -Path $TempZip -Force -ErrorAction SilentlyContinue
        exit 1
    }
    Write-Host "  Checksum verified"
}

$TempDir = Join-Path $env:TEMP "taurine-ext-$([guid]::NewGuid())"
$ExtractJob = {
    param($zip, $dest)
    Expand-Archive -Path $zip -DestinationPath $dest -Force
    return $dest
}
Invoke-WithRetry -ScriptBlock $ExtractJob -ArgumentList @($TempZip, $TempDir) -Label "Extracting" | Out-Null

if (-not (Test-Path $InstallDir)) {
    New-Item -ItemType Directory -Path $InstallDir | Out-Null
}

Copy-Item -Path (Join-Path $TempDir "taurine.exe") -Destination $InstallDir -Force

Remove-Item -Path $TempZip -Force
Remove-Item -Path $TempDir -Recurse -Force

# Add to PATH if not present
$PathRegKey = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey("Environment", $true)
$CurrentPath = $PathRegKey.GetValue("Path", $null, "DoNotExpandEnvironmentNames")

if (-not $CurrentPath.Contains($InstallDir)) {
    $NewPath = if ($CurrentPath.EndsWith(";")) { "$CurrentPath$InstallDir" } else { "$CurrentPath;$InstallDir" }
    $PathRegKey.SetValue("Path", $NewPath, [Microsoft.Win32.RegistryValueKind]::ExpandString)
    
    # Broadcast WM_SETTINGCHANGE
    $Signature = @'
[DllImport("user32.dll", SetLastError = true, CharSet = CharSet.Auto)]
public static extern IntPtr SendMessageTimeout(
    IntPtr hWnd, uint Msg, UIntPtr wParam, string lParam,
    uint fuFlags, uint uTimeout, out UIntPtr lpdwResult);
'@
    $User32 = Add-Type -MemberDefinition $Signature -Name "User32" -Namespace "Win32" -PassThru
    $HWND_BROADCAST = [IntPtr]0xffff
    $WM_SETTINGCHANGE = 0x001A
    $SMTO_ABORTIFHUNG = 0x0002
    
    $result = [UIntPtr]::Zero
    $User32::SendMessageTimeout($HWND_BROADCAST, $WM_SETTINGCHANGE, [UIntPtr]::Zero, "Environment", $SMTO_ABORTIFHUNG, 5000, [ref]$result) | Out-Null
    
    Write-Host "PATH updated. You may need to restart your terminal to use taurine directly."
}

Write-Host -ForegroundColor Green "✓ taurine v$Version installed"