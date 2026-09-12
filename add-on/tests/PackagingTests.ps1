$ErrorActionPreference = 'Stop'
$addonRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
$packagePath = Join-Path ([IO.Path]::GetTempPath()) ('lychee-dev-package-' + [guid]::NewGuid().ToString('N') + '.zip')
$archive = $null
try {
    & (Join-Path $addonRoot 'tools/Package.ps1') -OutputPath $packagePath | Out-Null
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $archive = [IO.Compression.ZipFile]::OpenRead($packagePath)
    $names = @($archive.Entries | ForEach-Object { $_.FullName })
    foreach ($name in $names) {
        if (-not $name.StartsWith('Lychee Dev/') -or $name -match '(^|/)(add-on|tests|tools|docs|Analyze|publish|Lychee Dev skill)/' -or $name -match 'Performance\.lua$') {
            throw "Unexpected package entry: $name"
        }
    }
    foreach ($toc in @('Lychee Dev_Mainline.toc', 'Lychee Dev_Mists.toc', 'Lychee Dev_Wrath.toc')) {
        if ($names -notcontains "Lychee Dev/$toc") { throw "Missing packaged TOC: $toc" }
        foreach ($line in Get-Content -LiteralPath (Join-Path $addonRoot $toc)) {
            $relative = $line.Trim().Replace('\', '/')
            if ($relative -and -not $relative.StartsWith('#') -and $names -notcontains "Lychee Dev/$relative") {
                throw "Packaged TOC has a missing dependency: $relative"
            }
        }
    }
    if ($names -notcontains 'Lychee Dev/Media/Logo.png') { throw 'Package is missing runtime media' }
    $autoEntry = $archive.GetEntry('Lychee Dev/Modules/Automation/auto/auto.lua')
    if ($null -eq $autoEntry) { throw 'Package is missing the automation task registry' }
    $reader = [IO.StreamReader]::new($autoEntry.Open())
    try {
        $autoText = $reader.ReadToEnd()
    } finally {
        $reader.Dispose()
    }
    if ($autoText -match '(?m)^-- BEGIN LYCHEE DEV TASK \S') { throw 'Package ships local task source in the automation registry' }
    if ($autoText -notmatch 'ns\.AutomationTaskDefinitions\s*=\s*ns\.AutomationTaskDefinitions\s*or\s*\{\}') { throw 'Package automation registry does not declare an empty registration table' }
    Write-Output "Lychee Dev packaging tests passed ($($names.Count) runtime files)"
} finally {
    if ($archive) { $archive.Dispose() }
    if (Test-Path -LiteralPath $packagePath) { Remove-Item -LiteralPath $packagePath }
}
