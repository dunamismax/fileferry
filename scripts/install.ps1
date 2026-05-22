[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$Archive,

    [string]$InstallDir = $(if ($env:FILEFERRY_INSTALL_DIR) {
            $env:FILEFERRY_INSTALL_DIR
        } elseif ($env:USERPROFILE) {
            Join-Path $env:USERPROFILE ".local/bin"
        } else {
            Join-Path $HOME ".local/bin"
        }),

    [string]$ChecksumFile,
    [string]$Checksum,
    [switch]$NoChecksum,
    [switch]$DryRun
)

$ErrorActionPreference = "Stop"

function Fail([string]$Message) {
    throw "install.ps1: $Message"
}

function Info([string]$Message) {
    [Console]::Error.WriteLine("install.ps1: $Message")
}

function Resolve-RequiredPath([string]$Path, [string]$Label) {
    if ([string]::IsNullOrWhiteSpace($Path)) {
        Fail "$Label is required"
    }

    $resolved = Resolve-Path -LiteralPath $Path -ErrorAction SilentlyContinue
    if (-not $resolved) {
        Fail "$Label not found: $Path"
    }

    $resolved.ProviderPath
}

function Convert-ToTarPath([string]$Path) {
    if (-not $IsWindows) {
        return $Path
    }

    $normalized = $Path
    if ($normalized.StartsWith("\\?\UNC\")) {
        $normalized = "\\" + $normalized.Substring(8)
    } elseif ($normalized.StartsWith("\\?\")) {
        $normalized = $normalized.Substring(4)
    }

    $cygpath = Get-Command cygpath -ErrorAction SilentlyContinue
    if ($cygpath) {
        $converted = & $cygpath.Source -u $normalized 2>$null
        if ($LASTEXITCODE -eq 0 -and $converted) {
            return ($converted | Select-Object -First 1).Trim()
        }
    }

    $normalized
}

function Find-ChecksumInFile([string]$Path, [string]$ArchiveName) {
    foreach ($line in Get-Content -LiteralPath $Path) {
        if ($line -match '^\s*([0-9A-Fa-f]{64})\s+(\*?)(.+?)\s*$') {
            $name = $Matches[3]
            if ($name -eq $ArchiveName) {
                return $Matches[1].ToLowerInvariant()
            }
        }
    }

    $null
}

function Verify-Checksum([string]$ArchivePath) {
    if ($NoChecksum -and ($Checksum -or $ChecksumFile)) {
        Fail "-NoChecksum cannot be combined with -Checksum or -ChecksumFile"
    }

    $archiveName = Split-Path -Leaf $ArchivePath
    $expected = $null

    if ($Checksum) {
        if ($Checksum -notmatch '^[0-9A-Fa-f]{64}$') {
            Fail "-Checksum must be a SHA-256 hex digest"
        }
        $expected = $Checksum.ToLowerInvariant()
    } else {
        $candidateChecksumFile = $ChecksumFile
        if (-not $candidateChecksumFile) {
            $candidate = Join-Path (Split-Path -Parent $ArchivePath) "SHA256SUMS"
            if (Test-Path -LiteralPath $candidate -PathType Leaf) {
                $candidateChecksumFile = $candidate
            }
        }

        if ($candidateChecksumFile) {
            $resolvedChecksumFile = Resolve-RequiredPath $candidateChecksumFile "checksum file"
            $expected = Find-ChecksumInFile $resolvedChecksumFile $archiveName
            if (-not $expected) {
                Fail "no checksum entry for $archiveName in $resolvedChecksumFile"
            }
        } elseif ($NoChecksum) {
            Info "checksum verification skipped"
            return
        } else {
            Info "warning: no checksum supplied; use -ChecksumFile, -Checksum, or -NoChecksum"
            return
        }
    }

    $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $ArchivePath).Hash.ToLowerInvariant()
    if ($actual -ne $expected) {
        Fail "checksum mismatch for $archiveName"
    }

    Info "verified SHA-256 for $archiveName"
}

$archivePath = Resolve-RequiredPath $Archive "archive"
$tempRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("fileferry-install-" + [System.Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $tempRoot | Out-Null

try {
    Verify-Checksum $archivePath

    & tar -xzf (Convert-ToTarPath $archivePath) -C (Convert-ToTarPath $tempRoot)
    if ($LASTEXITCODE -ne 0) {
        Fail "extract archive"
    }

    $binary = Get-ChildItem -LiteralPath $tempRoot -Recurse -File |
        Where-Object { $_.Name -eq "ferry" -or $_.Name -eq "ferry.exe" } |
        Sort-Object { if ($_.Name -eq "ferry") { 0 } else { 1 } }, FullName |
        Select-Object -First 1

    if (-not $binary) {
        Fail "archive does not contain a ferry binary"
    }

    $destination = Join-Path $InstallDir $binary.Name

    if ($DryRun) {
        Info "dry run: would install $($binary.FullName) to $destination"
        exit 0
    }

    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    Copy-Item -LiteralPath $binary.FullName -Destination $destination -Force

    if (-not $IsWindows) {
        & chmod 755 $destination
        if ($LASTEXITCODE -ne 0) {
            Fail "mark ferry executable"
        }
    }

    Info "installed ferry to $destination"
} finally {
    Remove-Item -LiteralPath $tempRoot -Recurse -Force -ErrorAction SilentlyContinue
}
