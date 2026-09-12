$ErrorActionPreference = "Stop"

# Ensure TLS 1.2 is enabled for secure downloads
[System.Net.ServicePointManager]::SecurityProtocol = [System.Net.ServicePointManager]::SecurityProtocol -bor [System.Net.SecurityProtocolType]::Tls12

$Platform = "windows-x86_64"
$MaxRetries = 3
$RetryDelay = 2

function Show-SpinnerJob ($Job, $Label, $SuccessLabel = $null) {
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

    $jobError = $Job.Error
    $jobState = $Job.State
    $result = Receive-Job -Job $Job -ErrorAction SilentlyContinue
    $hasError = ($null -ne $jobError -and $jobError.Count -gt 0) -or ($jobState -eq "Failed") -or ($result -eq $false)

    if ($hasError) {
        Write-Host -ForegroundColor Yellow -NoNewline "$([char]0x2713) "
        $DisplayLabel = $Label
    } else {
        Write-Host -ForegroundColor Green -NoNewline "$([char]0x2713) "
        $DisplayLabel = if ($null -eq $SuccessLabel) { $Label } else { $SuccessLabel }
    }

    $diff = $Label.Length - $DisplayLabel.Length
    if ($diff -gt 0) {
        $DisplayLabel = $DisplayLabel + (" " * $diff)
    }
    Write-Host $DisplayLabel
    Remove-Job -Job $Job
    return $result
}

function Invoke-WithRetry ($ScriptBlock, $ArgumentList, $Label, $SuccessLabel = $null) {
    $attempt = 0
    $delay = $RetryDelay
    while ($attempt -lt $MaxRetries) {
        $job = Start-Job -ScriptBlock $ScriptBlock -ArgumentList $ArgumentList
        $result = Show-SpinnerJob $job $Label $SuccessLabel
        $jobError = $job.Error
        $jobState = $job.State
        if ($jobState -eq "Failed" -or ($jobError -and $jobError.Count -gt 0)) {
            $attempt++
            if ($attempt -lt $MaxRetries) {
                Write-Host "  Retrying in ${delay}s... ($attempt/$MaxRetries)"
                Start-Sleep -Seconds $delay
                $delay *= 2
            }
            continue
        }
        # Only return if we have a meaningful result (not null, not empty string)
        if ($null -ne $result -and $result -ne '') {
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

function Format-Bytes {
    param([long]$Bytes)
    $inv = [Globalization.CultureInfo]::InvariantCulture
    if ($Bytes -lt 1024) { return "$Bytes B" }
    if ($Bytes -lt 1048576) { return ([string]::Format($inv, "{0:F1} KB", $Bytes / 1024)) }
    if ($Bytes -lt 1073741824) { return ([string]::Format($inv, "{0:F1} MB", $Bytes / 1048576)) }
    return ([string]::Format($inv, "{0:F1} GB", $Bytes / 1073741824))
}

function Format-Pair {
    param([long]$Downloaded, [long]$Total)
    $inv = [Globalization.CultureInfo]::InvariantCulture
    if ($Total -ge 1073741824) { $d = 1073741824; $u = "GB" }
    elseif ($Total -ge 1048576) { $d = 1048576; $u = "MB" }
    elseif ($Total -ge 1024) { $d = 1024; $u = "KB" }
    else { return "$Downloaded/$Total B" }
    return ([string]::Format($inv, "{0:F1}/{1:F1} {2}", $Downloaded / $d, $Total / $d, $u))
}

function Format-Speed {
    param([double]$BytesPerSec)
    # NB: [double]::IsFinite is .NET Core only — IsNaN/IsInfinity work on 5.1 too.
    if ([double]::IsNaN($BytesPerSec) -or [double]::IsInfinity($BytesPerSec) -or $BytesPerSec -le 0) { return "0 B/s" }
    return "$(Format-Bytes ([long]$BytesPerSec))/s"
}

function Format-Duration {
    param([long]$Seconds)
    if ($Seconds -lt 60) { return "${Seconds}s" }
    return "$([math]::Floor($Seconds / 60))m $($Seconds % 60)s"
}

function Format-DownloadLine {
    param($Downloaded, $Total, [double]$Speed)
    if ($null -eq $Total -or $Total -le 0) {
        return "  $(Format-Bytes ([long]$Downloaded)) downloaded @ $(Format-Speed $Speed)"
    }
    $pct = [math]::Floor([double]$Downloaded * 100 / $Total)
    if ($pct -gt 100) { $pct = 100 }
    if ($pct -lt 0) { $pct = 0 }
    return "  $(Format-Pair ([long]$Downloaded) ([long]$Total)) (${pct}%) @ $(Format-Speed $Speed)"
}

# Two-line download: line 1 keeps the label, line 2 shows live progress.
# Invoke-WebRequest cannot stream progress out of a Job, so this uses
# HttpClient directly (no auto-decompression: Content-Length == file bytes).
function Invoke-DownloadWithProgress {
    param([string]$Url, [string]$Out, [string]$Label, [string]$SuccessLabel = $null)
    if ($null -eq $SuccessLabel) { $SuccessLabel = $Label }
    $esc = [char]0x1b
    $redirected = [Console]::IsOutputRedirected
    # System.Net.Http is not loaded by default on Windows PowerShell 5.1.
    Add-Type -AssemblyName System.Net.Http -ErrorAction SilentlyContinue | Out-Null
    $spinstr = @(
        [char]0x280b, [char]0x2819, [char]0x2839, [char]0x2838, [char]0x283c,
        [char]0x2834, [char]0x2826, [char]0x2827, [char]0x2807, [char]0x280f
    )
    $client = New-Object System.Net.Http.HttpClient
    $client.Timeout = [TimeSpan]::FromSeconds(300)
    $sw = [Diagnostics.Stopwatch]::StartNew()
    if ($redirected) { Write-Host "$Label..." }
    try {
        $response = $client.GetAsync($Url, [System.Net.Http.HttpCompletionOption]::ResponseHeadersRead).GetAwaiter().GetResult()
        $response.EnsureSuccessStatusCode() | Out-Null
        $total = $response.Content.Headers.ContentLength
        $stream = $response.Content.ReadAsStreamAsync().GetAwaiter().GetResult()
        $file = [System.IO.File]::OpenWrite($Out)
        try {
            $buffer = New-Object byte[] 65536
            $downloaded = [long]0
            $winBytes = [long]0
            $speed = 0.0
            $winStartMs = 0
            $lastDrawMs = -1000
            $frameIdx = 0
            if (-not $redirected) { Write-Host "$Label" }
            while (($n = $stream.Read($buffer, 0, $buffer.Length)) -gt 0) {
                $file.Write($buffer, 0, $n)
                $downloaded += $n
                $winBytes += $n
                $ms = $sw.ElapsedMilliseconds
                if ($ms - $winStartMs -ge 1000) {
                    $speed = $winBytes * 1000.0 / ($ms - $winStartMs)
                    $winStartMs = $ms
                    $winBytes = 0
                } elseif ($speed -eq 0 -and $ms -gt 0) {
                    $speed = $downloaded * 1000.0 / $ms
                }
                if ((-not $redirected) -and ($ms - $lastDrawMs -ge 200)) {
                    $frame = $spinstr[$frameIdx % $spinstr.Length]
                    $frameIdx++
                    $line2 = Format-DownloadLine $downloaded $total $speed
                    Write-Host -NoNewline "${esc}[1A`r$frame $Label${esc}[K`n`r$line2${esc}[K"
                    $lastDrawMs = $ms
                }
            }
        } finally {
            if ($null -ne $file) { $file.Close() }
            if ($null -ne $stream) { $stream.Close() }
            if ($null -ne $response) { $response.Dispose() }
        }
        $secs = [long]$sw.Elapsed.TotalSeconds
        $size = Format-Bytes $downloaded
        $dur = Format-Duration $secs
        if (-not $redirected) { Write-Host -NoNewline "`r${esc}[K${esc}[1A`r${esc}[K" }
    } finally {
        $sw.Stop()
        $client.Dispose()
    }
    Write-Host -ForegroundColor Green -NoNewline "$([char]0x2713) "
    Write-Host "$SuccessLabel ($size in $dur)"
}

function Invoke-DownloadWithRetry {
    param([string]$Url, [string]$Out, [string]$Label, [string]$SuccessLabel = $null)
    $attempt = 0
    $delay = $RetryDelay
    while ($true) {
        $thisLabel = $Label
        if ($attempt -gt 0) { $thisLabel = "$Label (attempt $($attempt + 1)/$MaxRetries)" }
        try {
            Invoke-DownloadWithProgress -Url $Url -Out $Out -Label $thisLabel -SuccessLabel $SuccessLabel
            return
        } catch {
            $attempt++
            if ($attempt -ge $MaxRetries) { throw }
            Write-Host "  Download failed ($_). Retrying in ${delay}s... ($attempt/$MaxRetries)"
            Start-Sleep -Seconds $delay
            $delay *= 2
            if (Test-Path $Out) { Remove-Item $Out -Force -ErrorAction SilentlyContinue }
        }
    }
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

function Compare-Versions ($v1, $v2) {
    if ($v1 -eq $v2) { return 0 }
    
    $v1HasHyphen = $v1.Contains('-')
    $v2HasHyphen = $v2.Contains('-')
    
    $v1Base = Get-VersionBase $v1
    $v2Base = Get-VersionBase $v2
    
    $localBase = [version]$v1Base
    $remoteBase = [version]$v2Base
    
    if ($localBase -lt $remoteBase) { return -1 }
    if ($localBase -gt $remoteBase) { return 1 }
    
    if ($v1HasHyphen -and -not $v2HasHyphen) { return -1 }
    if (-not $v1HasHyphen -and $v2HasHyphen) { return 1 }
    
    $v1Suffix = $v1.Substring($v1.IndexOf('-') + 1)
    $v2Suffix = $v2.Substring($v2.IndexOf('-') + 1)
    
    $v1Parts = $v1Suffix -split '\.'
    $v2Parts = $v2Suffix -split '\.'
    
    $max = [Math]::Max($v1Parts.Length, $v2Parts.Length)
    for ($i = 0; $i -lt $max; $i++) {
        $p1 = if ($i -lt $v1Parts.Length) { $v1Parts[$i] } else { $null }
        $p2 = if ($i -lt $v2Parts.Length) { $v2Parts[$i] } else { $null }
        
        if ($null -eq $p1 -and $null -ne $p2) { return -1 }
        if ($null -ne $p1 -and $null -eq $p2) { return 1 }
        if ($p1 -eq $p2) { continue }
        
        $p1IsNum = $p1 -match '^\d+$'
        $p2IsNum = $p2 -match '^\d+$'
        
        if ($p1IsNum -and $p2IsNum) {
            $n1 = [int]$p1
            $n2 = [int]$p2
            if ($n1 -lt $n2) { return -1 }
            if ($n1 -gt $n2) { return 1 }
        } else {
            $cmp = [String]::Compare($p1, $p2, $true)
            if ($cmp -lt 0) { return -1 }
            if ($cmp -gt 0) { return 1 }
        }
    }
    return 0
}

function Main {
    $InstallDir = Join-Path $env:LOCALAPPDATA "Taurine\bin"
    $ExePath = Join-Path $InstallDir "taurine.exe"

    $IsInstalled = $false
    $IsFreshInstall = $false
    $LocalVersion = $null
    $Version = $null
    $Url = $null
    $Sha256 = $null

    # 1. Local Check First
    if (Test-Path $ExePath) {
        $IsInstalled = $true
        try {
            $versionOutput = & $ExePath --version 2>$null
            if ($versionOutput) {
                $LocalVersion = ($versionOutput -split " ")[1]
            }
        } catch {
            # --version flag not supported
        }

        if ($LocalVersion) {
            try {
                $Releases = Invoke-RestMethod -Uri "https://api.github.com/repos/ereinaimer/taurine/releases" -Headers @{ Accept = "application/vnd.github+json" } -ErrorAction SilentlyContinue -TimeoutSec 10
                if ($Releases -and $Releases.Count -gt 0) {
                    $LatestRelease = $Releases[0]
                    $ReleaseDetail = Invoke-RestMethod -Uri $LatestRelease.url -Headers @{ Accept = "application/vnd.github+json" } -ErrorAction SilentlyContinue -TimeoutSec 10
                    if ($ReleaseDetail -and $ReleaseDetail.assets) {
                        $ManifestAsset = $ReleaseDetail.assets | Where-Object { $_.name -eq "manifest.json" } | Select-Object -First 1
                        if ($ManifestAsset) {
                            $Manifest = Invoke-RestMethod -Uri $ManifestAsset.browser_download_url -ErrorAction SilentlyContinue -TimeoutSec 10
                            if ($Manifest -is [string]) {
                                $Manifest = $Manifest | ConvertFrom-Json
                            }
                            $Version = $Manifest.version
                            $Url = $Manifest.artifacts.$Platform.url
                            $Sha256 = $Manifest.artifacts.$Platform.sha256
                            # Handle malformed sha256 with filename prefix
                            if ($Sha256 -and $Sha256.Contains(':')) {
                                $Sha256 = $Sha256.Split(':')[-1]
                            }

                            if ($Version) {
                                $cmp = Compare-Versions $LocalVersion $Version
                                if ($cmp -ge 0) {
                                    Write-Host -ForegroundColor Green -NoNewline "$([char]0x2713) "
                                    Write-Host "Taurine is up to date (v$LocalVersion)"
                                }
                            }
                        }
                    }
                }
            } catch {
                # Fall back to standard flow
            }
        }
    }

    # 2. Manifest fetch if not already populated (e.g. fresh install or silent check failed)
    if ($null -eq $Version) {
        $ManifestJob = {
            param($Platform)
            $ErrorActionPreference = "Stop"
            $Releases = Invoke-RestMethod -Uri "https://api.github.com/repos/ereinaimer/taurine/releases" -Headers @{ Accept = "application/vnd.github+json" } -TimeoutSec 10
            if (-not $Releases -or $Releases.Count -eq 0) {
                throw "Could not find any releases."
            }
            $LatestRelease = $Releases[0]
            $ReleaseDetail = Invoke-RestMethod -Uri $LatestRelease.url -Headers @{ Accept = "application/vnd.github+json" } -TimeoutSec 10
            $ManifestAsset = $ReleaseDetail.assets | Where-Object { $_.name -eq "manifest.json" } | Select-Object -First 1
            if (-not $ManifestAsset) {
                throw "No manifest.json asset found in latest release."
            }
            $Manifest = Invoke-RestMethod -Uri $ManifestAsset.browser_download_url -TimeoutSec 10
            # Validate manifest structure
            if (-not $Manifest.version -or -not $Manifest.artifacts -or -not $Manifest.artifacts.$Platform -or -not $Manifest.artifacts.$Platform.url) {
                throw "Invalid manifest: missing required fields"
            }
            return $Manifest
        }
        $Manifest = Invoke-WithRetry -ScriptBlock $ManifestJob -ArgumentList @($Platform) -Label "Fetching release manifest" -SuccessLabel "Fetched release manifest"
        if ($Manifest -is [string]) {
            $Manifest = $Manifest | ConvertFrom-Json
        }
        $Version = $Manifest.version
        $Url = $Manifest.artifacts.$Platform.url
        $Sha256 = $Manifest.artifacts.$Platform.sha256
        # Handle malformed sha256 with filename prefix (e.g. "checksums/file.sha256:hash")
        if ($Sha256 -and $Sha256.Contains(':')) {
            $Sha256 = $Sha256.Split(':')[-1]
        }

        if (-not $Version -or -not $Url) {
            Write-Host -ForegroundColor Red "Error: Could not determine latest version or download URL."
            throw "Could not determine latest version or download URL."
        }
    }

    # 3. Handle already installed but outdated/failed checks
    if ($IsInstalled) {
        if ($LocalVersion) {
            if ($Version) {
                if ((Compare-Versions $LocalVersion $Version) -lt 0) {
                    Write-Host "A newer version of Taurine (v$Version) is available. Please run 'tau update' to update."
                }
            } else {
                Write-Host "Taurine is already installed. If you want to update to the latest version, please run 'tau update'."
            }
        } else {
            Write-Host "Taurine is already installed. If you want to update to the latest version (v$Version), please run 'tau update'."
        }
    }

    $TempZip = $null
    $TempDir = $null

    if (-not $IsInstalled) {
        try {
            $TempZip = Join-Path $env:TEMP "taurine-$([guid]::NewGuid()).zip"

            # Download archive with retry and live two-line progress
            Invoke-DownloadWithRetry -Url $Url -Out $TempZip -Label "Downloading taurine v$Version" -SuccessLabel "Downloaded taurine v$Version"

            # Verify checksum if available
            if ($Sha256) {
                $ChecksumJob = {
                    param($zip, $expected)
                    $ErrorActionPreference = "Stop"
                    try {
                        $computed = (Get-FileHash -Path $zip -Algorithm SHA256).Hash.ToLower()
                        return ($computed -eq $expected.ToLower())
                    } catch {
                        throw "Checksum verification failed: $_"
                    }
                }
                $job = Start-Job -ScriptBlock $ChecksumJob -ArgumentList @($TempZip, $Sha256)
                $result = Show-SpinnerJob $job "Verifying checksum" "Verified checksum"
                if ($result -ne $true) {
                    Write-Host -ForegroundColor Red "Error: Checksum mismatch for downloaded archive."
                    throw "Checksum mismatch for downloaded archive."
                }
            }

            $TempDir = Join-Path $env:TEMP "taurine-ext-$([guid]::NewGuid())"
            $ExtractJob = {
                param($zip, $dest)
                $ErrorActionPreference = "Stop"
                Expand-Archive -Path $zip -DestinationPath $dest -Force
                return $dest
            }
            Invoke-WithRetry -ScriptBlock $ExtractJob -ArgumentList @($TempZip, $TempDir) -Label "Extracting" -SuccessLabel "Extracted" | Out-Null

            if (-not (Test-Path $InstallDir)) {
                New-Item -ItemType Directory -Path $InstallDir | Out-Null
            }

            Copy-Item -Path (Join-Path $TempDir "taurine.exe") -Destination $InstallDir -Force

            # Download uninstall.ps1 script silently in the background
            $UninstallScriptPath = Join-Path $InstallDir "uninstall.ps1"
            try {
                $null = Start-Job -ScriptBlock {
                    param($url, $out)
                    $ErrorActionPreference = "Stop"
                    Invoke-WebRequest -Uri $url -OutFile $out -UseBasicParsing -TimeoutSec 30
                } -ArgumentList @("https://raw.githubusercontent.com/ereinaimer/taurine/main/uninstall.ps1", $UninstallScriptPath)
            } catch {}

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
            if (-not ([System.Management.Automation.PSTypeName]'Win32.User32').Type) {
                Add-Type -MemberDefinition $Signature -Name "User32" -Namespace "Win32" | Out-Null
            }
            $User32 = [Win32.User32]
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
                Write-Host -ForegroundColor Green -NoNewline "$([char]0x2713) "
                Write-Host "PATH updated in registry."
                Write-Host -ForegroundColor Yellow -NoNewline "$([char]0x2713) "
                Write-Host "You may need to restart your terminal to use taurine directly."
            } elseif ($PathInEnv) {
                Write-Host -ForegroundColor Green -NoNewline "$([char]0x2713) "
                Write-Host "Taurine binary is already on your PATH."
            } else {
                Write-Host -ForegroundColor Green -NoNewline "$([char]0x2713) "
                Write-Host "Taurine binary is configured in registry PATH."
                Write-Host -ForegroundColor Yellow -NoNewline "$([char]0x2713) "
                Write-Host "Please restart your terminal to apply the change."
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
            Write-Host -ForegroundColor Green -NoNewline "$([char]0x2713) "
            if ($AliasUpdated) {
                Write-Host "Added alias 'tau' to your PowerShell profile(s)."
            } else {
                Write-Host "alias 'tau' is already set up in your profile(s)."
            }
            Write-Host "Now you can run 'tau --help' for more details."
        }

        if ($IsFreshInstall) {
            try {
                if (Test-Path $ExePath) {
                    Start-Process -FilePath $ExePath -ArgumentList "up" -WindowStyle Hidden -ErrorAction Stop
                }
            } catch {
                Write-Host "Warning: Failed to start Taurine service automatically."
            }
        }

        # Write registry uninstall keys to register in Add or Remove Programs
        $UninstallScriptPath = Join-Path $InstallDir "uninstall.ps1"
        try {
            $UninstallKeyPath = "Software\Microsoft\Windows\CurrentVersion\Uninstall\Taurine"
            $UninstallKey = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey($UninstallKeyPath)
            $UninstallKey.SetValue("DisplayName", "Taurine")
            $UninstallKey.SetValue("DisplayVersion", $Manifest.version)
            $UninstallKey.SetValue("Publisher", "Erein Aimer")
            $UninstallKey.SetValue("InstallLocation", $InstallDir)
            $UninstallKey.SetValue("DisplayIcon", $ExePath)
            $UninstallKey.SetValue("UninstallString", "powershell.exe -NoProfile -ExecutionPolicy Bypass -File `"$UninstallScriptPath`"")
        } catch {
            Write-Host "Warning: Failed to register Taurine in Add or Remove Programs."
        }
    }
}

Main