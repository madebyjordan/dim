param(
    [string]$VsWherePath = "",
    [switch]$IgnoreSystemDiscovery
)

$ErrorActionPreference = "Stop"

function Complete-Detection {
    param([string]$Status, [string]$Detail)
    Write-Output "$Status|$Detail"
    exit 0
}

try {
    $vswhereCandidates = @()
    if ($VsWherePath) {
        $vswhereCandidates += $VsWherePath
    }
    if (-not $IgnoreSystemDiscovery) {
        $command = Get-Command vswhere.exe -ErrorAction SilentlyContinue
        if ($command) {
            $vswhereCandidates += $command.Source
        }
        if (${env:ProgramFiles(x86)}) {
            $vswhereCandidates += Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
        }
        if ($env:ProgramFiles) {
            $vswhereCandidates += Join-Path $env:ProgramFiles "Microsoft Visual Studio\Installer\vswhere.exe"
        }
    }

    $vswhere = $vswhereCandidates | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
    $installations = @()
    $vcComponentRegistered = $false
    if ($vswhere) {
        $global:LASTEXITCODE = 0
        $json = & $vswhere -all -products * -format json -utf8
        if ($LASTEXITCODE -ne 0) {
            Complete-Detection "inconclusive" "vswhere.exe could not enumerate Visual Studio installations"
        }
        if ($json) {
            $installations = @($json | ConvertFrom-Json | ForEach-Object { $_.installationPath } | Where-Object { $_ })
        }
        $registered = & $vswhere -all -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
        $vcComponentRegistered = [bool]$registered
    }

    if (-not $IgnoreSystemDiscovery) {
        if ($env:VSINSTALLDIR) {
            $installations += $env:VSINSTALLDIR
        }
        if ($env:ProgramFiles) {
            $visualStudioRoot = Join-Path $env:ProgramFiles "Microsoft Visual Studio\2022"
            if (Test-Path $visualStudioRoot) {
                $installations += Get-ChildItem $visualStudioRoot -Directory -ErrorAction SilentlyContinue | ForEach-Object { $_.FullName }
            }
        }
    }
    $installations = @($installations | Where-Object { $_ -and (Test-Path $_) } | Select-Object -Unique)
    $setupInstancesExist = -not $IgnoreSystemDiscovery -and (Test-Path "HKLM:\SOFTWARE\Microsoft\VisualStudio\Setup\Instances")

    $compiler = if ($IgnoreSystemDiscovery) { $null } else { Get-Command cl.exe -ErrorAction SilentlyContinue }
    if (-not $compiler) {
        foreach ($installation in $installations) {
            $compiler = Get-ChildItem (Join-Path $installation "VC\Tools\MSVC\*\bin\Hostx64\x64\cl.exe") -File -ErrorAction SilentlyContinue |
                Sort-Object FullName -Descending |
                Select-Object -First 1
            if ($compiler) { break }
        }
    }

    $sdkRoots = @()
    if ($env:WindowsSdkDir) {
        $sdkRoots += $env:WindowsSdkDir
    }
    if (-not $IgnoreSystemDiscovery) {
        $kits = Get-ItemProperty "HKLM:\SOFTWARE\Microsoft\Windows Kits\Installed Roots" -ErrorAction SilentlyContinue
        if ($kits -and $kits.KitsRoot10) {
            $sdkRoots += $kits.KitsRoot10
        }
    }
    $sdkRoots = @($sdkRoots | Where-Object { $_ -and (Test-Path $_) } | Select-Object -Unique)

    $sdkReady = $false
    foreach ($sdkRoot in $sdkRoots) {
        $windowsHeader = Get-ChildItem (Join-Path $sdkRoot "Include\*\um\Windows.h") -File -ErrorAction SilentlyContinue | Select-Object -First 1
        $kernelLibrary = Get-ChildItem (Join-Path $sdkRoot "Lib\*\um\x64\kernel32.lib") -File -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($windowsHeader -and $kernelLibrary) {
            $sdkReady = $true
            break
        }
    }

    if ($compiler -and $sdkReady) {
        Complete-Detection "ready" "MSVC compiler and Windows SDK detected"
    }
    if ($compiler -and -not $sdkReady) {
        Complete-Detection "missing-sdk" "MSVC compiler detected, but no usable Windows SDK headers and x64 libraries were found"
    }
    if ($installations.Count -gt 0 -and -not $compiler -and -not $vcComponentRegistered) {
        Complete-Detection "missing-vctools" "Visual Studio detected, but the MSVC x64 compiler and VCTools component were not found"
    }
    if ($installations.Count -gt 0 -or $vcComponentRegistered) {
        Complete-Detection "inconclusive" "Visual Studio components were registered, but the MSVC compiler could not be verified"
    }
    if ($vswhere) {
        Complete-Detection "missing-build-tools" "vswhere.exe found no Visual Studio installation with an MSVC compiler"
    }
    if ($setupInstancesExist) {
        Complete-Detection "inconclusive" "Visual Studio setup metadata exists, but its installation and MSVC compiler could not be verified"
    }
    Complete-Detection "missing-build-tools" "No Visual Studio installation or MSVC compiler was found"
} catch {
    Complete-Detection "inconclusive" "Windows toolchain detection failed: $($_.Exception.Message)"
}
