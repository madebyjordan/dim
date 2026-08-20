param(
    [Parameter(Mandatory = $true)]
    [string] $RepositoryRoot,

    [string] $ShimDirectory = "",

    [ValidateSet("User", "Process")]
    [string] $PathTarget = "User",

    [switch] $ForceShim
)

$ErrorActionPreference = "Stop"

function Resolve-PathEntry([string] $Entry) {
    if ([string]::IsNullOrWhiteSpace($Entry)) {
        return $null
    }
    $expanded = [Environment]::ExpandEnvironmentVariables($Entry.Trim().Trim('"'))
    try {
        return [IO.Path]::GetFullPath($expanded).TrimEnd('\')
    } catch {
        return $expanded.TrimEnd('\')
    }
}

function Test-PathListContains([string] $PathList, [string] $Directory) {
    $expected = Resolve-PathEntry $Directory
    foreach ($entry in @($PathList -split ';')) {
        $resolved = Resolve-PathEntry $entry
        if ($resolved -and $resolved.Equals($expected, [StringComparison]::OrdinalIgnoreCase)) {
            return $true
        }
    }
    return $false
}

function Test-CorepackPnpmShim([string] $CommandPath) {
    if (-not (Test-Path -LiteralPath $CommandPath -PathType Leaf)) {
        return $false
    }
    try {
        return (Get-Content -Raw -LiteralPath $CommandPath) -match '[\\/]corepack[\\/].*pnpm'
    } catch {
        return $false
    }
}

function Get-PnpmVersion([string] $CommandPath) {
    Push-Location -LiteralPath $RepositoryRoot
    try {
        $output = & $CommandPath --version 2>&1
        if ($LASTEXITCODE -ne 0) {
            return $null
        }
        return (($output | Out-String).Trim())
    } finally {
        Pop-Location
    }
}

function Get-PnpmVersionFromPersistedPath {
    $machinePath = [Environment]::GetEnvironmentVariable('Path', 'Machine')
    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    $persistedPath = [Environment]::ExpandEnvironmentVariables("$machinePath;$userPath")
    $previousPath = $env:Path
    $env:Path = $persistedPath
    Push-Location -LiteralPath $RepositoryRoot
    try {
        $output = & "$env:SystemRoot\System32\cmd.exe" /d /c "pnpm --version" 2>&1
        if ($LASTEXITCODE -ne 0) {
            return $null
        }
        return (($output | Out-String).Trim())
    } finally {
        Pop-Location
        $env:Path = $previousPath
    }
}

function Remove-ManagedPowerShellShims([string] $Directory) {
    # PowerShell prefers .ps1 over .cmd. Keeping the Corepack-generated CMD shim avoids failures on
    # machines whose execution policy blocks unsigned local PowerShell scripts.
    foreach ($scriptShim in @('pnpm.ps1', 'pnpx.ps1')) {
        $scriptPath = Join-Path $Directory $scriptShim
        if (Test-Path -LiteralPath $scriptPath -PathType Leaf) {
            Remove-Item -LiteralPath $scriptPath -Force
        }
    }
}

function Find-PersistedCorepackPnpm([string] $ExpectedVersion) {
    $persistedPath = @(
        [Environment]::GetEnvironmentVariable('Path', 'Machine'),
        [Environment]::GetEnvironmentVariable('Path', 'User')
    ) -join ';'
    foreach ($entry in @($persistedPath -split ';')) {
        $directory = Resolve-PathEntry $entry
        if (-not $directory) { continue }
        $candidate = Join-Path $directory 'pnpm.cmd'
        if ((Test-CorepackPnpmShim $candidate) -and
            (Get-PnpmVersion $candidate) -eq $ExpectedVersion) {
            return $candidate
        }
    }
    return $null
}

function Publish-EnvironmentChange {
    if (-not ('Eclipse.EnvironmentChange' -as [type])) {
        Add-Type @'
using System;
using System.Runtime.InteropServices;

namespace Eclipse {
    public static class EnvironmentChange {
        [DllImport("user32.dll", SetLastError = true, CharSet = CharSet.Auto)]
        public static extern IntPtr SendMessageTimeout(
            IntPtr hWnd,
            uint message,
            UIntPtr wParam,
            string lParam,
            uint flags,
            uint timeout,
            out UIntPtr result
        );
    }
}
'@
    }
    $result = [UIntPtr]::Zero
    [void][Eclipse.EnvironmentChange]::SendMessageTimeout(
        [IntPtr]0xffff,
        0x001A,
        [UIntPtr]::Zero,
        'Environment',
        0x0002,
        5000,
        [ref]$result
    )
}

$manifestPath = Join-Path $RepositoryRoot 'package.json'
if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) {
    throw "Repository package manifest not found: $manifestPath"
}
$packageManager = (Get-Content -Raw -LiteralPath $manifestPath | ConvertFrom-Json).packageManager
if ($packageManager -notmatch '^pnpm@(.+)$') {
    throw "The repository packageManager must pin pnpm; found '$packageManager'."
}
$expectedVersion = $Matches[1]

if (-not $ShimDirectory) {
    if ($env:LOCALAPPDATA) {
        $ShimDirectory = Join-Path $env:LOCALAPPDATA 'Eclipse\bin'
    } else {
        $ShimDirectory = Join-Path $env:USERPROFILE '.eclipse\bin'
    }
}
$ShimDirectory = [IO.Path]::GetFullPath($ShimDirectory)
$requestedShim = Join-Path $ShimDirectory 'pnpm.cmd'
$shimReady = $false

if (-not $ForceShim) {
    if ((Test-CorepackPnpmShim $requestedShim) -and
        (Get-PnpmVersion $requestedShim) -eq $expectedVersion) {
        $shimReady = $true
        Remove-ManagedPowerShellShims $ShimDirectory
        $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
        $machinePath = [Environment]::GetEnvironmentVariable('Path', 'Machine')
        if ($PathTarget -eq 'Process' -or
            (Test-PathListContains $userPath $ShimDirectory) -or
            (Test-PathListContains $machinePath $ShimDirectory)) {
            if ($PathTarget -eq 'User' -and
                (Get-PnpmVersionFromPersistedPath) -ne $expectedVersion) {
                throw "The persisted Windows PATH does not resolve repository-pinned pnpm $expectedVersion."
            }
            Write-Output "ready|$expectedVersion|$requestedShim|existing"
            exit 0
        }
    } elseif ($PathTarget -eq 'User') {
        $existingShim = Find-PersistedCorepackPnpm $expectedVersion
        if ($existingShim) {
            if ((Get-PnpmVersionFromPersistedPath) -ne $expectedVersion) {
                throw "The persisted Windows PATH does not resolve repository-pinned pnpm $expectedVersion."
            }
            Write-Output "ready|$expectedVersion|$existingShim|existing"
            exit 0
        }
    }
}

if (-not $shimReady) {
    $corepack = Get-Command corepack.cmd -ErrorAction SilentlyContinue
    if (-not $corepack) {
        $corepack = Get-Command corepack -ErrorAction SilentlyContinue
    }
    if (-not $corepack) {
        throw 'Corepack is unavailable; pnpm shims cannot be prepared.'
    }

    New-Item -ItemType Directory -Path $ShimDirectory -Force | Out-Null
    $corepackOutput = & $corepack.Source enable pnpm --install-directory $ShimDirectory 2>&1
    if ($LASTEXITCODE -ne 0) {
        throw "Corepack could not create user-level pnpm shims in '$ShimDirectory': $($corepackOutput | Out-String)"
    }

    Remove-ManagedPowerShellShims $ShimDirectory
}

if (-not (Test-CorepackPnpmShim $requestedShim)) {
    throw "Corepack did not create the expected pnpm command at '$requestedShim'."
}
if ((Get-PnpmVersion $requestedShim) -ne $expectedVersion) {
    throw "The pnpm shim did not resolve repository-pinned pnpm $expectedVersion."
}

if ($PathTarget -eq 'User') {
    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    if (-not (Test-PathListContains $userPath $ShimDirectory)) {
        $newUserPath = if ([string]::IsNullOrWhiteSpace($userPath)) {
            $ShimDirectory
        } else {
            "$ShimDirectory;$userPath"
        }
        try {
            [Environment]::SetEnvironmentVariable('Path', $newUserPath, 'User')
            Publish-EnvironmentChange
        } catch {
            throw "The pnpm shim was created, but its user PATH entry could not be persisted: $($_.Exception.Message)"
        }
    }
    if ((Get-PnpmVersionFromPersistedPath) -ne $expectedVersion) {
        throw "The pnpm shim was prepared, but a fresh Windows command session does not resolve repository-pinned pnpm $expectedVersion."
    }
}

$env:Path = "$ShimDirectory;$env:Path"
Write-Output "ready|$expectedVersion|$requestedShim|prepared"
