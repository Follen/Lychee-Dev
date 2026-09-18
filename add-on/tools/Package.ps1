[CmdletBinding()]
param(
    [string]$OutputPath,
    [switch]$Force
)

$ErrorActionPreference = 'Stop'
$addonRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
$files = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
# Discover every client selector so adding a client cannot silently ship without
# its TOC; PackagingTests asserts the expected set is present.
$tocs = @(Get-ChildItem -LiteralPath $addonRoot -File -Filter 'Lychee Dev_*.toc' |
    Sort-Object -Property Name | Select-Object -ExpandProperty Name)
if ($tocs.Count -eq 0) { throw 'No client TOC found; refusing to build an empty package' }
$version = $null

foreach ($toc in $tocs) {
    $tocPath = Join-Path $addonRoot $toc
    $files.Add($tocPath) | Out-Null
    foreach ($line in Get-Content -LiteralPath $tocPath -Encoding UTF8) {
        if ($line -match '^## Version:\s*(.+)$') {
            $tocVersion = $Matches[1].Trim()
            if ($version -and $version -ne $tocVersion) { throw 'TOC versions differ' }
            $version = $tocVersion
        }
        $relative = $line.Trim()
        if (-not $relative -or $relative.StartsWith('#')) { continue }
        $path = [IO.Path]::GetFullPath((Join-Path $addonRoot $relative))
        if (-not $path.StartsWith($addonRoot + '\', [StringComparison]::OrdinalIgnoreCase)) {
            throw "TOC path escapes the addon: $relative"
        }
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Missing TOC file: $relative" }
        # Current TOCs list Lua directly; fail if a future XML closure needs packaging support.
        if ([IO.Path]::GetExtension($path) -ne '.lua') { throw "Unsupported TOC entry: $relative" }
        $files.Add($path) | Out-Null
    }
}
if (-not $version -or $version -notmatch '^[0-9A-Za-z._-]+$') { throw 'Invalid package version' }
foreach ($file in Get-ChildItem -LiteralPath (Join-Path $addonRoot 'Media') -File -Recurse) {
    $files.Add($file.FullName) | Out-Null
}

if (-not $OutputPath) { $OutputPath = Join-Path $addonRoot "publish/Lychee Dev-$version.zip" }
$destination = [IO.Path]::GetFullPath($OutputPath)
if ((Test-Path -LiteralPath $destination) -and -not $Force) { throw 'Package exists; pass -Force to replace it' }
$directory = Split-Path -Parent $destination
New-Item -ItemType Directory -Path $directory -Force | Out-Null
Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem
$stream = [IO.File]::Open($destination, [IO.FileMode]::Create, [IO.FileAccess]::Write)
$archive = $null
try {
    $archive = [IO.Compression.ZipArchive]::new($stream, [IO.Compression.ZipArchiveMode]::Create)
    foreach ($file in @($files | Sort-Object)) {
        $relative = $file.Substring($addonRoot.Length + 1).Replace('\', '/')
        [IO.Compression.ZipFileExtensions]::CreateEntryFromFile($archive, $file,
            "Lychee Dev/$relative", [IO.Compression.CompressionLevel]::Optimal) | Out-Null
    }
} finally {
    if ($archive) { $archive.Dispose() }
    $stream.Dispose()
}
Write-Output $destination
