[CmdletBinding()]
param([switch]$Force)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$repositoryRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot ".."))
$resourceRoot = [IO.Path]::GetFullPath((Join-Path $repositoryRoot "src-tauri\resources"))
$targetDirectory = [IO.Path]::GetFullPath((Join-Path $resourceRoot "kokoro-en-v0_19"))
$manifestPath = Join-Path $targetDirectory "ASSET-MANIFEST.json"
$noticePath = Join-Path $targetDirectory "SANYMAR-THIRD-PARTY-NOTICE.md"
$manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json

if (-not $targetDirectory.StartsWith($resourceRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
    throw "Refusing to prepare a model outside Sanymar's resource directory."
}

$cacheDirectory = [IO.Path]::GetFullPath((Join-Path $repositoryRoot ".local\kokoro-assets"))
New-Item -ItemType Directory -Force -Path $cacheDirectory | Out-Null
$staleBefore = (Get-Date).AddMinutes(-5)
Get-ChildItem -LiteralPath $cacheDirectory -Directory -Filter "extract-*" -ErrorAction SilentlyContinue |
    Where-Object { $_.LastWriteTime -lt $staleBefore } |
    ForEach-Object {
        $staleDirectory = [IO.Path]::GetFullPath($_.FullName)
        if ($staleDirectory.StartsWith($cacheDirectory + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
            Remove-Item -LiteralPath $staleDirectory -Recurse -Force
        }
    }

function Test-FileHash {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$Expected
    )

    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        return $false
    }
    $actual = (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash
    return $actual.Equals($Expected, [StringComparison]::OrdinalIgnoreCase)
}

function Test-KokoroDirectory {
    param([Parameter(Mandatory = $true)][string]$Directory)

    foreach ($property in $manifest.requiredFiles.PSObject.Properties) {
        if (-not (Test-FileHash -Path (Join-Path $Directory $property.Name) -Expected $property.Value)) {
            return $false
        }
    }
    $espeakDirectory = Join-Path $Directory "espeak-ng-data"
    return (Test-Path -LiteralPath $espeakDirectory -PathType Container) -and
        ((Get-ChildItem -LiteralPath $espeakDirectory -File -Recurse).Count -ge 300)
}

if (-not $Force -and (Test-KokoroDirectory -Directory $targetDirectory)) {
    Write-Host "Pinned Kokoro assets are present and verified."
    exit 0
}

$archivePath = Join-Path $cacheDirectory "kokoro-en-v0_19.tar.bz2"
$downloadPath = "$archivePath.download"

$archiveIsValid = (Test-Path -LiteralPath $archivePath -PathType Leaf) -and
    ((Get-Item -LiteralPath $archivePath).Length -eq [long]$manifest.archiveBytes) -and
    (Test-FileHash -Path $archivePath -Expected $manifest.archiveSha256)

if (-not $archiveIsValid) {
    Remove-Item -LiteralPath $downloadPath -Force -ErrorAction SilentlyContinue
    Write-Host "Downloading pinned Kokoro build asset..."
    & curl.exe --fail --location --retry 3 --output $downloadPath $manifest.archiveUrl
    if ($LASTEXITCODE -ne 0) {
        throw "Kokoro asset download failed with curl exit code $LASTEXITCODE."
    }
    if ((Get-Item -LiteralPath $downloadPath).Length -ne [long]$manifest.archiveBytes -or
        -not (Test-FileHash -Path $downloadPath -Expected $manifest.archiveSha256)) {
        Remove-Item -LiteralPath $downloadPath -Force -ErrorAction SilentlyContinue
        throw "Downloaded Kokoro archive failed its pinned size or SHA-256 check."
    }
    Move-Item -LiteralPath $downloadPath -Destination $archivePath -Force
}

$git = Get-Command git.exe -ErrorAction SilentlyContinue
if (-not $git) {
    throw "Git for Windows is required to extract the pinned Kokoro build asset."
}
$gitRoot = Split-Path (Split-Path $git.Source -Parent) -Parent
$gitToolDirectory = Join-Path $gitRoot "usr\bin"
$gitTarPath = Join-Path $gitToolDirectory "tar.exe"
$gitBzipPath = Join-Path $gitToolDirectory "bzip2.exe"
if (-not (Test-Path -LiteralPath $gitTarPath -PathType Leaf) -or
    -not (Test-Path -LiteralPath $gitBzipPath -PathType Leaf)) {
    throw "Git for Windows' matched usr\bin\tar.exe and bzip2.exe are required to extract the pinned Kokoro build asset."
}
$env:PATH = "$gitToolDirectory;$env:PATH"

$stagingDirectory = Join-Path $cacheDirectory ("extract-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $stagingDirectory | Out-Null
try {
    & $gitTarPath --force-local -xjf $archivePath -C $stagingDirectory
    if ($LASTEXITCODE -ne 0) {
        throw "Kokoro archive extraction failed with tar exit code $LASTEXITCODE."
    }
    $extractedDirectory = Join-Path $stagingDirectory $manifest.asset
    if (-not (Test-KokoroDirectory -Directory $extractedDirectory)) {
        throw "Extracted Kokoro assets failed required-file verification."
    }

    New-Item -ItemType Directory -Force -Path $targetDirectory | Out-Null
    Copy-Item -Path (Join-Path $extractedDirectory "*") -Destination $targetDirectory -Recurse -Force
    if (-not (Test-KokoroDirectory -Directory $targetDirectory)) {
        throw "Prepared Kokoro resource directory failed final verification."
    }
    if (-not (Test-Path -LiteralPath $noticePath -PathType Leaf)) {
        throw "Sanymar's Kokoro third-party notice is missing."
    }
    Write-Host "Pinned Kokoro assets downloaded, extracted, and verified."
}
finally {
    if ($stagingDirectory.StartsWith($cacheDirectory + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
        Remove-Item -LiteralPath $stagingDirectory -Recurse -Force -ErrorAction SilentlyContinue
    }
}
