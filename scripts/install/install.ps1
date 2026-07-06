$ErrorActionPreference = "Stop"

$Platform = "windows-x86_64"

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

$ManifestJob = Start-Job -ScriptBlock {
    Invoke-RestMethod -Uri "https://github.com/ereinaimer/taurine/releases/latest/download/manifest.json"
}
$Manifest = Show-SpinnerJob $ManifestJob "Fetching latest release manifest"
$Version = $Manifest.version
$Url = $Manifest.artifacts.$Platform.url

if (-not $Version -or -not $Url) {
    Write-Error "Could not determine latest version or download URL."
    exit 1
}

$TempZip = Join-Path $env:TEMP "taurine-$([guid]::NewGuid()).zip"
$DownloadJob = Start-Job -ScriptBlock {
    param($url, $out)
    Invoke-WebRequest -Uri $url -OutFile $out -UseBasicParsing
} -ArgumentList $Url, $TempZip
Show-SpinnerJob $DownloadJob "Downloading taurine v$Version" | Out-Null

$TempDir = Join-Path $env:TEMP "taurine-ext-$([guid]::NewGuid())"
$ExtractJob = Start-Job -ScriptBlock {
    param($zip, $dest)
    Expand-Archive -Path $zip -DestinationPath $dest -Force
} -ArgumentList $TempZip, $TempDir
Show-SpinnerJob $ExtractJob "Extracting" | Out-Null

$InstallDir = Join-Path $env:LOCALAPPDATA "Taurine\bin"
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
