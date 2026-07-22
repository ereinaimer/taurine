$ErrorActionPreference = "Continue"

# Ensure TLS 1.2 is enabled
[System.Net.ServicePointManager]::SecurityProtocol = [System.Net.ServicePointManager]::SecurityProtocol -bor [System.Net.SecurityProtocolType]::Tls12

$SpinFrames = @(
    [char]0x280b, [char]0x2819, [char]0x2839, [char]0x2838, [char]0x283c,
    [char]0x2834, [char]0x2826, [char]0x2827, [char]0x2807, [char]0x280f
)

function Run-Step ($Label, [scriptblock]$Action) {
    $job = Start-Job -ScriptBlock $Action
    $i = 0
    while ($job.State -eq "Running") {
        $frame = $SpinFrames[$i % $SpinFrames.Length]
        Write-Host -NoNewline "`r$frame $Label"
        $i++
        Start-Sleep -Milliseconds 80
    }
    Write-Host -NoNewline "`r"

    $jobError = $job.Error
    $jobState = $job.State
    Receive-Job -Job $job -ErrorAction SilentlyContinue | Out-Null
    $hasError = ($null -ne $jobError -and $jobError.Count -gt 0) -or ($jobState -eq "Failed")
    Remove-Job -Job $job

    if ($hasError) {
        Write-Host -ForegroundColor Yellow -NoNewline "$([char]0x2713) "
    } else {
        Write-Host -ForegroundColor Green -NoNewline "$([char]0x2713) "
    }
    Write-Host $Label
}

$InstallDir = Join-Path $env:LOCALAPPDATA "Taurine\bin"
$ExePath = Join-Path $InstallDir "taurine.exe"
$DataDir = Join-Path $env:LOCALAPPDATA "Taurine"

# Stop service and kill leftover processes
Run-Step "Stopping Taurine service" {
    $exe = Join-Path $env:LOCALAPPDATA "Taurine\bin\taurine.exe"
    if (Test-Path $exe) {
        try { & $exe down | Out-Null } catch {}
    }
    Stop-Process -Name "taurine" -Force -ErrorAction SilentlyContinue
}

# Uninstall shell completions
Run-Step "Removing shell completions" {
    $exe = Join-Path $env:LOCALAPPDATA "Taurine\bin\taurine.exe"
    if (Test-Path $exe) {
        try { & $exe completions uninstall | Out-Null } catch {}
    }
}

# Remove bin directory from Registry PATH and broadcast change
Run-Step "Removing PATH entry" {
    $InstallDir = Join-Path $env:LOCALAPPDATA "Taurine\bin"
    $PathRegKey = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey("Environment", $true)
    if ($null -ne $PathRegKey) {
        $CurrentPath = $PathRegKey.GetValue("Path", $null, "DoNotExpandEnvironmentNames")
        if ($null -ne $CurrentPath) {
            $Paths = $CurrentPath -split ';' | Where-Object { $_.TrimEnd('\') -ne $InstallDir.TrimEnd('\') }
            $NewPath = $Paths -join ';'
            $PathRegKey.SetValue("Path", $NewPath, [Microsoft.Win32.RegistryValueKind]::ExpandString)

            $Signature = @'
[DllImport("user32.dll", SetLastError = true, CharSet = CharSet.Auto)]
public static extern IntPtr SendMessageTimeout(
    IntPtr hWnd, uint Msg, UIntPtr wParam, string lParam,
    uint fuFlags, uint uTimeout, out UIntPtr lpdwResult);
'@
            try {
                if (-not ([System.Management.Automation.PSTypeName]'Win32.User32').Type) {
                    Add-Type -MemberDefinition $Signature -Name "User32" -Namespace "Win32" | Out-Null
                }
                $User32 = [Win32.User32]
                $result = [UIntPtr]::Zero
                $User32::SendMessageTimeout([IntPtr]0xffff, 0x001A, [UIntPtr]::Zero, "Environment", 0x0002, 5000, [ref]$result) | Out-Null
            } catch {}
        }
    }
}

# Remove tau function from PowerShell profiles
Run-Step "Cleaning PowerShell profiles" {
    $ProfilesToUpdate = @()
    if ($PROFILE) {
        $ProfilesToUpdate += $PROFILE
        $allHosts = $PROFILE.CurrentUserAllHosts
        if ($allHosts -and ($ProfilesToUpdate -notcontains $allHosts)) {
            $ProfilesToUpdate += $allHosts
        }
    }
    $DocsDir = [Environment]::GetFolderPath("MyDocuments")
    if (-not $DocsDir -and $env:USERPROFILE) { $DocsDir = Join-Path $env:USERPROFILE "Documents" }
    if ($DocsDir) {
        $CommonProfiles = @(
            (Join-Path $DocsDir "PowerShell\Microsoft.PowerShell_profile.ps1"),
            (Join-Path $DocsDir "PowerShell\profile.ps1"),
            (Join-Path $DocsDir "WindowsPowerShell\Microsoft.PowerShell_profile.ps1"),
            (Join-Path $DocsDir "WindowsPowerShell\profile.ps1")
        )
        foreach ($p in $CommonProfiles) {
            if ($ProfilesToUpdate -notcontains $p) { $ProfilesToUpdate += $p }
        }
    }
    foreach ($profilePath in $ProfilesToUpdate) {
        if (Test-Path $profilePath) {
            $lines = Get-Content $profilePath -ErrorAction SilentlyContinue
            if ($null -ne $lines) {
                $matchingLines = @($lines | Where-Object { $_ -match '^\s*function\s+tau\b' })
                if ($matchingLines.Count -gt 0) {
                    $newLines = $lines | Where-Object { $_ -notmatch '^\s*function\s+tau\b' }
                    $newContent = ($newLines -join "`r`n").Trim()
                    if ($newContent) {
                        Set-Content -Path $profilePath -Value "$newContent`r`n" -Force
                    } else {
                        Remove-Item -Path $profilePath -Force
                    }
                }
            }
        }
    }
}

# Delete registry uninstall entry
Run-Step "Removing registry entry" {
    $UninstallKeyPath = "Software\Microsoft\Windows\CurrentVersion\Uninstall"
    $UninstallKey = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($UninstallKeyPath, $true)
    if ($null -ne $UninstallKey) {
        $UninstallKey.DeleteSubKeyTree("Taurine", $false)
    }
}

# Remove all configured API keys from OS keyring
$exe = Join-Path $env:LOCALAPPDATA "Taurine\bin\taurine.exe"
if (Test-Path $exe) {
    try { & $exe ai remove --all --yes --json | Out-Null } catch {}
}

# Delete all data (config, database, logs, binary) via background process to avoid file locking
Run-Step "Removing Taurine files" {
    $DataDir = Join-Path $env:LOCALAPPDATA "Taurine"
    $cleanupCmd = "Start-Sleep -Seconds 1; Remove-Item -Path '$DataDir' -Recurse -Force -ErrorAction SilentlyContinue"
    Start-Process powershell.exe -ArgumentList "-NoProfile -Command $cleanupCmd" -WindowStyle Hidden
}

Write-Host -ForegroundColor Green "Taurine uninstalled successfully."
