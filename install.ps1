$ErrorActionPreference = "Stop"

# Ensure TLS 1.2 is enabled for secure downloads
[System.Net.ServicePointManager]::SecurityProtocol = [System.Net.ServicePointManager]::SecurityProtocol -bor [System.Net.SecurityProtocolType]::Tls12

$Platform = "windows-x86_64"
$MaxRetries = 3
$RetryDelay = 2

function Show-SpinnerJob ($Job, $Label) {
    $spinstr = @(
        [char]0x280b, [char]0x2819, [char]0x2839, [char]0x2838, [char]0x283c,
        [char]0x2834, [char]0x2826, [char]0x2827, [char]0x2807, [char]0x280f
    )
    $i = 0
    while ($Job.State -eq "Running") {
        $frame = $spinstr[$i % $spinstr.Length]
        Write-Host -NoNewline "`r$frame $Label"
        $i++
        Start-Sleep -Milliseconds 80
    }
    Write-Host -NoNewline "`r"
    Write-Host -ForegroundColor Green -NoNewline "$([char]0x2713) "
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
    throw "Failed after $MaxRetries attempts: $Label"
}

function Get-VersionBase {
    param([string]$v)
    $idx = $v.IndexOf('-')
    if ($idx -ge 0) { return $v.Substring(0, $idx) } else { return $v }
}

function Update-PowerShellProfile {
    param([string]$ProfilePath)

    $tauFuncLine = "function tau { taurine @args }"
    $modified = $false

    if (Test-Path $ProfilePath) {
        $lines = Get-Content $ProfilePath -ErrorAction SilentlyContinue
        $matchingLines = @($lines | Where-Object { $_ -match '^\s*function\s+tau\b' })

        if ($matchingLines.Count -eq 1 -and $matchingLines[0].Trim() -eq $tauFuncLine) {
            # Already set up perfectly
            return $false
        }

        # Filter out all function tau lines and rewrite profile with exactly one correct function tau
        $newLines = $lines | Where-Object { $_ -notmatch '^\s*function\s+tau\b' }
        $newContent = ($newLines -join "`r`n").Trim()
        if ($newContent) {
            $newContent = "$newContent`r`n`r`n$tauFuncLine"
        } else {
            $newContent = "$tauFuncLine"
        }
        Set-Content -Path $ProfilePath -Value $newContent -Force
        $modified = $true
    } else {
        # Profile file does not exist. Create parent directory and set file contents.
        $parentDir = Split-Path $ProfilePath -Parent
        if (-not (Test-Path $parentDir)) {
            New-Item -ItemType Directory -Path $parentDir -Force | Out-Null
        }
        Set-Content -Path $ProfilePath -Value "`n$tauFuncLine`n" -NoNewLine
        $modified = $true
    }

    return $modified
}

function Main {
    $InstallDir = Join-Path $env:LOCALAPPDATA "Taurine\bin"
    $ExePath = Join-Path $InstallDir "taurine.exe"

    # Fetch manifest with retry
    $ManifestJob = {
        param($url)
        Invoke-RestMethod -Uri $url
    }
    $Manifest = Invoke-WithRetry -ScriptBlock $ManifestJob -ArgumentList @("https://github.com/ereinaimer/taurine/releases/latest/download/manifest.json") -Label "Fetching latest release manifest"
    if ($Manifest -is [string]) {
        $Manifest = $Manifest | ConvertFrom-Json
    }
    $Version = $Manifest.version
    $Url = $Manifest.artifacts.$Platform.url
    $Sha256 = $Manifest.artifacts.$Platform.sha256

    if (-not $Version -or -not $Url) {
        Write-Host -ForegroundColor Red "Error: Could not determine latest version or download URL."
        throw "Could not determine latest version or download URL."
    }

    $IsInstalled = $false
    $IsFreshInstall = $false

    if (Test-Path $ExePath) {
        $LocalVersion = $null
        try {
            $versionOutput = & $ExePath --version 2>$null
            if ($versionOutput) {
                $LocalVersion = ($versionOutput -split " ")[1]
            }
        } catch {
            # --version flag not supported
        }

        if ($LocalVersion) {
            if ($LocalVersion -eq $Version) {
                Write-Host -ForegroundColor Green -NoNewline "$([char]0x2713) "
                Write-Host "Taurine is already installed, up to date (v$LocalVersion), and configured properly."
            } else {
                try {
                    $localBase = [version](Get-VersionBase $LocalVersion)
                    $remoteBase = [version](Get-VersionBase $Version)
                    if ($localBase -lt $remoteBase) {
                        Write-Host "A newer version of Taurine (v$Version) is available. Please run 'tau update' to update."
                    } else {
                        Write-Host -ForegroundColor Green -NoNewline "$([char]0x2713) "
                        Write-Host "Taurine is already installed, up to date (v$LocalVersion), and configured properly."
                    }
                } catch {
                    Write-Host "A newer version of Taurine (v$Version) is available. Please run 'tau update' to update."
                }
            }
        } else {
            Write-Host "Taurine is already installed. If you want to update to the latest version (v$Version), please run 'tau update'."
        }
        $IsInstalled = $true
    }

    $TempZip = $null
    $TempDir = $null

    if (-not $IsInstalled) {
        try {
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
                $ChecksumJob = {
                    param($zip, $expected)
                    $computed = (Get-FileHash -Path $zip -Algorithm SHA256).Hash.ToLower()
                    return ($computed -eq $expected.ToLower())
                }
                $job = Start-Job -ScriptBlock $ChecksumJob -ArgumentList @($TempZip, $Sha256)
                $result = Show-SpinnerJob $job "Verifying checksum"
                if ($result -isnot [bool] -or -not $result) {
                    if ($result -isnot [bool]) {
                        Write-Host -ForegroundColor Red "Error: Checksum verification tool failed for downloaded archive."
                        throw "Checksum verification tool failed for downloaded archive."
                    }
                    Write-Host -ForegroundColor Red "Error: Checksum mismatch for downloaded archive."
                    throw "Checksum mismatch for downloaded archive."
                }
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
            $IsInstalled = $true
            $IsFreshInstall = $true

            Write-Host -ForegroundColor Green -NoNewline "$([char]0x2713) "
            Write-Host "taurine v$Version installed"
        } finally {
            if ($TempZip -and (Test-Path $TempZip)) {
                Remove-Item -Path $TempZip -Force -ErrorAction SilentlyContinue
            }
            if ($TempDir -and (Test-Path $TempDir)) {
                Remove-Item -Path $TempDir -Recurse -Force -ErrorAction SilentlyContinue
            }
        }
    }

    if ($IsInstalled) {
        # Add to PATH if not present (case-insensitive on Windows)
        $PathRegKey = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey("Environment", $true)
        $CurrentPath = $PathRegKey.GetValue("Path", $null, "DoNotExpandEnvironmentNames")
        $PathUpdated = $false

        if ($null -eq $CurrentPath -or $CurrentPath.IndexOf($InstallDir, [StringComparison]::OrdinalIgnoreCase) -lt 0) {
            if ($null -eq $CurrentPath) {
                $NewPath = $InstallDir
            } elseif ($CurrentPath.EndsWith(";")) {
                $NewPath = "$CurrentPath$InstallDir"
            } else {
                $NewPath = "$CurrentPath;$InstallDir"
            }
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
            $PathUpdated = $true
        }

        # Check if currently on environment PATH
        $PathInEnv = ($env:PATH -split ';' | ForEach-Object { $_.TrimEnd('\') }) -contains $InstallDir.TrimEnd('\')

        if ($IsFreshInstall) {
            if ($PathUpdated) {
                Write-Host "PATH updated in registry. You may need to restart your terminal to use taurine directly."
            } elseif ($PathInEnv) {
                Write-Host -ForegroundColor Green -NoNewline "$([char]0x2713) "
                Write-Host "Taurine binary is already on your PATH."
            } else {
                Write-Host "Taurine binary is configured in registry PATH, but not in your current shell session. Please restart your terminal."
            }
        }

        # Configure PowerShell Profiles
        $ProfilesToUpdate = @()
        if ($PROFILE) {
            $ProfilesToUpdate += $PROFILE
            $allHosts = $PROFILE.CurrentUserAllHosts
            if ($allHosts -and ($ProfilesToUpdate -notcontains $allHosts)) {
                $ProfilesToUpdate += $allHosts
            }
        }

        # Check other common PowerShell locations if parent directories exist
        $DocsDir = [Environment]::GetFolderPath("MyDocuments")
        if (-not $DocsDir -and $env:USERPROFILE) {
            $DocsDir = Join-Path $env:USERPROFILE "Documents"
        }
        if ($DocsDir) {
            $CommonProfiles = @(
                (Join-Path $DocsDir "PowerShell\Microsoft.PowerShell_profile.ps1"),
                (Join-Path $DocsDir "PowerShell\profile.ps1"),
                (Join-Path $DocsDir "WindowsPowerShell\Microsoft.PowerShell_profile.ps1"),
                (Join-Path $DocsDir "WindowsPowerShell\profile.ps1")
            )
            foreach ($p in $CommonProfiles) {
                if ($ProfilesToUpdate -notcontains $p) {
                    $parent = Split-Path $p -Parent
                    if (Test-Path $parent) {
                        $ProfilesToUpdate += $p
                    }
                }
            }
        }

        $AliasUpdated = $false
        foreach ($profilePath in $ProfilesToUpdate) {
            if (Update-PowerShellProfile -ProfilePath $profilePath) {
                $AliasUpdated = $true
            }
        }

        if ($IsFreshInstall) {
            if ($AliasUpdated) {
                Write-Host "Added or updated alias tau in your PowerShell profile(s)."
            } else {
                Write-Host -ForegroundColor Green -NoNewline "$([char]0x2713) "
                Write-Host "alias 'tau' is already set up in your profile(s)."
            }
            Write-Host "Now you can run 'tau --help' for more details."
        }
    }
}

Main