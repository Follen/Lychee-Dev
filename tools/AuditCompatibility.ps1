[CmdletBinding()]
param(
    [string]$AddonPath,
    [string]$OutputPath,
    [switch]$SkipWowdoc
)

$ErrorActionPreference = 'Stop'

$auditSchema = 'lychee.compatibility-audit.v1'
$sourceId = 'wow-ui-source'
$projectRoot = (Resolve-Path -LiteralPath (Split-Path -Parent $PSScriptRoot)).Path
$AddonPath = if ($AddonPath) { $AddonPath } else { $projectRoot }
$targetRoot = (Resolve-Path -LiteralPath $AddonPath).Path
$useProjectPolicy = $targetRoot.Equals($projectRoot, [System.StringComparison]::OrdinalIgnoreCase)
$tempRoot = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
$stageRoot = Join-Path $tempRoot ('lychee-compat-' + [guid]::NewGuid().ToString('N'))

$clients = @(
    [pscustomobject]@{
        id = 'retail'
        product = 'retail'
        ref = '12.1.0'
        commit = '31c7f7b9cc79e56c986b365c06a6afbcf3c9177b'
        interface = '120100'
        toc = 'Lychee Dev_Mainline.toc'
        tocSuffix = '_Mainline.toc'
        profile = 'Core/Clients/Mainline.lua'
        catalog = 'Modules/Events/CatalogData_Mainline.lua'
    },
    [pscustomobject]@{
        id = 'classic'
        product = 'classic'
        ref = '5.5.4'
        commit = '1028c1e687f721ba9d3af14d1b12a5745e4227c7'
        interface = '50504'
        toc = 'Lychee Dev_Mists.toc'
        tocSuffix = '_Mists.toc'
        profile = 'Core/Clients/Mists.lua'
        catalog = 'Modules/Events/CatalogData_Mists.lua'
    },
    [pscustomobject]@{
        id = 'titan'
        product = 'titan'
        ref = '3.80.2'
        commit = '825d29d3662b372f0bead725ee6abd339e4a77b5'
        interface = '38002'
        toc = 'Lychee Dev_Wrath.toc'
        tocSuffix = '_Wrath.toc'
        profile = 'Core/Clients/Titan.lua'
        catalog = 'Modules/Events/CatalogData_Titan.lua'
    }
)

$findings = [System.Collections.Generic.List[object]]::new()

function Add-Finding {
    param(
        [string]$Id,
        [ValidateSet('error', 'warning', 'info')][string]$Severity,
        [string]$Client,
        [string]$File,
        [int]$Line,
        [string]$Message,
        [string]$Evidence
    )
    $findings.Add([pscustomobject]@{
        id = $Id
        severity = $Severity
        client = $Client
        file = $File
        line = $Line
        message = $Message
        evidence = $Evidence
    })
}

function Convert-ToRelativePath {
    param([string]$Root, [string]$Path)
    $rootWithSeparator = $Root.TrimEnd('\', '/') + [System.IO.Path]::DirectorySeparatorChar
    $rootUri = [uri]$rootWithSeparator
    $pathUri = [uri]$Path
    return [uri]::UnescapeDataString($rootUri.MakeRelativeUri($pathUri).ToString()).Replace('\', '/')
}

function Resolve-LoadPath {
    param([string]$Root, [string]$BaseDirectory, [string]$RelativePath)
    $normalized = $RelativePath.Replace('/', [System.IO.Path]::DirectorySeparatorChar)
    $candidate = [System.IO.Path]::GetFullPath((Join-Path $BaseDirectory $normalized))
    $rootPrefix = $Root.TrimEnd('\', '/') + [System.IO.Path]::DirectorySeparatorChar
    if (-not $candidate.StartsWith($rootPrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "Load path escapes addon root: $RelativePath"
    }
    return $candidate
}

function Read-Toc {
    param([string]$Path)
    $metadata = [ordered]@{}
    $files = [System.Collections.Generic.List[string]]::new()
    foreach ($rawLine in Get-Content -LiteralPath $Path) {
        $line = $rawLine.TrimEnd("`r")
        if ($line -match '^##\s+([^:]+):\s*(.*?)\s*$') {
            $metadata[$matches[1]] = $matches[2]
        } elseif ($line -and -not $line.StartsWith('#')) {
            $files.Add($line.Replace('\', '/'))
        }
    }
    return [pscustomobject]@{ metadata = $metadata; files = @($files) }
}

function Find-ClientToc {
    param([string]$Root, [object]$Client, [bool]$UseProjectPolicy)
    if ($UseProjectPolicy) {
        return $Client.toc
    }
    $matches = @(Get-ChildItem -LiteralPath $Root -File -Filter "*$($Client.tocSuffix)")
    if ($matches.Count -eq 1) {
        return $matches[0].Name
    }
    if ($matches.Count -gt 1) {
        Add-Finding -Id 'ambiguous-toc' -Severity error -Client $Client.id -File '' -Line 0 `
            -Message 'More than one TOC matches the client suffix.' `
            -Evidence ($matches.Name -join ',')
    }
    return $null
}

function Get-LoadClosure {
    param([string]$Root, [string[]]$Entries, [string]$ClientId)
    $queue = [System.Collections.Generic.Queue[object]]::new()
    foreach ($entry in $Entries) {
        $queue.Enqueue([pscustomobject]@{ base = $Root; path = $entry })
    }
    $seen = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::OrdinalIgnoreCase)
    $ordered = [System.Collections.Generic.List[string]]::new()

    while ($queue.Count -gt 0) {
        $item = $queue.Dequeue()
        try {
            $absolute = Resolve-LoadPath -Root $Root -BaseDirectory $item.base -RelativePath $item.path
        } catch {
            Add-Finding -Id 'load-path-escape' -Severity error -Client $ClientId -File $item.path -Line 0 `
                -Message $_.Exception.Message -Evidence 'TOC/XML load closure'
            continue
        }
        $relative = Convert-ToRelativePath -Root $Root -Path $absolute
        if (-not $seen.Add($relative)) {
            continue
        }
        if (-not (Test-Path -LiteralPath $absolute -PathType Leaf)) {
            Add-Finding -Id 'missing-load-file' -Severity error -Client $ClientId -File $relative -Line 0 `
                -Message 'The TOC/XML load closure references a missing file.' -Evidence $item.path
            continue
        }
        $ordered.Add($relative)

        if ([System.IO.Path]::GetExtension($absolute) -ieq '.xml') {
            try {
                [xml]$document = Get-Content -LiteralPath $absolute -Raw
                $base = Split-Path -Parent $absolute
                foreach ($node in $document.SelectNodes('//*[@file]')) {
                    $queue.Enqueue([pscustomobject]@{ base = $base; path = [string]$node.file })
                }
            } catch {
                Add-Finding -Id 'invalid-xml' -Severity error -Client $ClientId -File $relative -Line 0 `
                    -Message 'The loaded XML file could not be parsed.' -Evidence $_.Exception.Message
            }
        }
    }
    return @($ordered)
}

function Read-EventCatalog {
    param([string]$Path)
    $events = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::Ordinal)
    foreach ($line in Get-Content -LiteralPath $Path) {
        if ($line -match '^\s*"([A-Z][A-Z0-9_]+)",\s*"') {
            [void]$events.Add($matches[1])
        }
    }
    return $events
}

function Test-LuaSource {
    param([string]$Root, [string[]]$Files, [object]$Client, [object]$Catalog)
    $literalCount = 0
    $dynamicCount = 0
    $branchPatterns = @(
        'WOW_PROJECT_(?:ID|MAINLINE|MISTS_CLASSIC|WRATH_CLASSIC|CATACLYSM_CLASSIC)',
        'LE_EXPANSION_LEVEL_',
        '\bC_GameRules\b'
    )
    $legacyApiPattern = '(?<![\w.])(?:GetNumAddOns|GetAddOnInfo|GetAddOnMetadata|IsAddOnLoaded|GetMouseFocus)\s*\('

    foreach ($relative in $Files) {
        if ([System.IO.Path]::GetExtension($relative) -ine '.lua') {
            continue
        }
        $absolute = Join-Path $Root $relative
        $lineNumber = 0
        foreach ($line in Get-Content -LiteralPath $absolute) {
            $lineNumber++
            $eventMatches = [regex]::Matches($line,
                '(?:RegisterEvent|RegisterUnitEvent)\s*\(\s*["'']([A-Z][A-Z0-9_]+)["'']')
            foreach ($match in $eventMatches) {
                $literalCount++
                $eventName = $match.Groups[1].Value
                if (-not $Catalog.Contains($eventName)) {
                    Add-Finding -Id 'event-unavailable' -Severity error -Client $Client.id -File $relative `
                        -Line $lineNumber -Message "Event $eventName is not documented for this client ref." `
                        -Evidence "$($Client.product) $($Client.ref) $($Client.commit)"
                }
            }
            if ($line -match '(?:RegisterEvent|RegisterUnitEvent)\s*\(' -and $eventMatches.Count -eq 0) {
                $dynamicCount++
            }

            $isBoundaryFile = $relative -eq 'Core/Compatibility.lua' -or $relative.StartsWith('Core/Clients/')
            if (-not $isBoundaryFile) {
                foreach ($pattern in $branchPatterns) {
                    if ($line -match $pattern) {
                        Add-Finding -Id 'scattered-product-branch' -Severity warning -Client $Client.id `
                            -File $relative -Line $lineNumber `
                            -Message 'Client-product branching is outside the compatibility boundary.' `
                            -Evidence $matches[0]
                    }
                }
                if ($line -match $legacyApiPattern) {
                    Add-Finding -Id 'legacy-addon-api' -Severity warning -Client $Client.id -File $relative `
                        -Line $lineNumber -Message 'Legacy addon API access is outside Core/Compatibility.lua.' `
                        -Evidence $matches[0]
                }
            }
        }
    }
    return [pscustomobject]@{ literalEvents = $literalCount; dynamicEventRegistrations = $dynamicCount }
}

function Invoke-WowdocValidation {
    param([string]$Path, [object]$Client)
    $raw = & wowdoc validate --path $Path --source $sourceId --product $Client.product --ref $Client.ref
    if ($LASTEXITCODE -ne 0) {
        throw "wowdoc validate failed for $($Client.id)"
    }
    $result = ($raw | Out-String) | ConvertFrom-Json
    if (-not $result.ok) {
        throw "wowdoc validate returned an unsuccessful result for $($Client.id)"
    }
    return $result.data
}

$clientResults = [System.Collections.Generic.List[object]]::new()
$sharedOrder = $null

try {
    New-Item -ItemType Directory -Path $stageRoot | Out-Null

    foreach ($client in $clients) {
        $tocName = Find-ClientToc -Root $targetRoot -Client $client -UseProjectPolicy $useProjectPolicy
        if (-not $tocName) {
            Add-Finding -Id 'missing-toc' -Severity error -Client $client.id -File "*$($client.tocSuffix)" -Line 0 `
                -Message 'The required client TOC is missing.' -Evidence $client.ref
            continue
        }
        $tocPath = Join-Path $targetRoot $tocName
        if (-not (Test-Path -LiteralPath $tocPath -PathType Leaf)) {
            Add-Finding -Id 'missing-toc' -Severity error -Client $client.id -File $tocName -Line 0 `
                -Message 'The required client TOC is missing.' -Evidence $client.ref
            continue
        }

        $toc = Read-Toc -Path $tocPath
        if ($toc.metadata.Interface -ne $client.interface) {
            Add-Finding -Id 'interface-mismatch' -Severity error -Client $client.id -File $tocName -Line 1 `
                -Message 'The TOC Interface does not match the supported client baseline.' `
                -Evidence "expected=$($client.interface); actual=$($toc.metadata.Interface)"
        }
        if ($useProjectPolicy) {
            if ($toc.files.Count -lt 2 -or $toc.files[0] -ne $client.profile `
                -or $toc.files[1] -ne 'Core/Compatibility.lua') {
                Add-Finding -Id 'compatibility-load-order' -Severity error -Client $client.id -File $tocName -Line 0 `
                    -Message 'The client profile and compatibility layer are not the first two loaded files.' `
                    -Evidence "expected=$($client.profile),Core/Compatibility.lua"
            }
            $catalogEntries = @($toc.files | Where-Object { $_ -like 'Modules/Events/CatalogData_*.lua' })
            if ($catalogEntries.Count -ne 1 -or $catalogEntries[0] -ne $client.catalog) {
                Add-Finding -Id 'event-catalog-mismatch' -Severity error -Client $client.id -File $tocName -Line 0 `
                    -Message 'The TOC does not load exactly one matching generated event catalog.' `
                    -Evidence "expected=$($client.catalog); actual=$($catalogEntries -join ',')"
            }

            $normalizedOrder = @($toc.files | ForEach-Object {
                if ($_ -like 'Core/Clients/*.lua') { 'Core/Clients/<client>.lua' }
                elseif ($_ -like 'Modules/Events/CatalogData_*.lua') { 'Modules/Events/CatalogData_<client>.lua' }
                else { $_ }
            }) -join "`n"
            if ($null -eq $sharedOrder) {
                $sharedOrder = $normalizedOrder
            } elseif ($normalizedOrder -ne $sharedOrder) {
                Add-Finding -Id 'shared-load-order-diverged' -Severity error -Client $client.id -File $tocName -Line 0 `
                    -Message 'Shared TOC load order differs between clients.' -Evidence 'Normalized TOC file list'
            }
        }

        $closure = Get-LoadClosure -Root $targetRoot -Entries $toc.files -ClientId $client.id
        $eventCatalog = Read-EventCatalog -Path (Join-Path $projectRoot $client.catalog)
        $sourceStats = Test-LuaSource -Root $targetRoot -Files $closure -Client $client -Catalog $eventCatalog

        $clientStage = Join-Path $stageRoot $client.id
        New-Item -ItemType Directory -Path $clientStage | Out-Null
        Copy-Item -LiteralPath $tocPath -Destination (Join-Path $clientStage $tocName)
        foreach ($relative in $closure) {
            $sourcePath = Join-Path $targetRoot $relative
            $destinationPath = Join-Path $clientStage $relative
            $destinationDirectory = Split-Path -Parent $destinationPath
            if (-not (Test-Path -LiteralPath $destinationDirectory)) {
                New-Item -ItemType Directory -Path $destinationDirectory -Force | Out-Null
            }
            Copy-Item -LiteralPath $sourcePath -Destination $destinationPath
        }

        $wowdocResult = $null
        $wowdocDiagnostics = [System.Collections.Generic.List[object]]::new()
        if (-not $SkipWowdoc) {
            try {
                $wowdocResult = Invoke-WowdocValidation -Path $clientStage -Client $client
                if ($null -ne $wowdocResult.diagnostics `
                    -and @($wowdocResult.diagnostics.PSObject.Properties).Count -gt 0) {
                    foreach ($diagnostic in @($wowdocResult.diagnostics)) {
                        $wowdocDiagnostics.Add($diagnostic)
                    }
                }
                if (-not $wowdocResult.valid) {
                    foreach ($diagnostic in $wowdocDiagnostics) {
                        Add-Finding -Id 'wowdoc-diagnostic' -Severity error -Client $client.id `
                            -File ([string]$diagnostic.file) -Line ([int]$diagnostic.line) `
                            -Message ([string]$diagnostic.message) -Evidence "$sourceId/$($client.product)@$($client.ref)"
                    }
                }
            } catch {
                Add-Finding -Id 'wowdoc-failed' -Severity error -Client $client.id -File $tocName -Line 0 `
                    -Message $_.Exception.Message -Evidence "$sourceId/$($client.product)@$($client.ref)"
            }
        }

        $clientResults.Add([pscustomobject]@{
            id = $client.id
            toc = $tocName
            interface = $client.interface
            officialSource = [pscustomobject]@{
                sourceId = $sourceId
                product = $client.product
                tag = $client.ref
                commit = $client.commit
            }
            loadClosureFiles = $closure.Count
            luaFiles = @($closure | Where-Object { $_ -like '*.lua' }).Count
            literalEvents = $sourceStats.literalEvents
            dynamicEventRegistrations = $sourceStats.dynamicEventRegistrations
            wowdoc = if ($wowdocResult) {
                [pscustomobject]@{
                    valid = [bool]$wowdocResult.valid
                    checkedLua = [int]$wowdocResult.checkedLua
                    diagnosticCount = $wowdocDiagnostics.Count
                    diagnostics = [object[]]@($wowdocDiagnostics)
                }
            } else { $null }
        })
    }
} finally {
    $resolvedStage = [System.IO.Path]::GetFullPath($stageRoot)
    if ($resolvedStage.StartsWith($tempRoot, [System.StringComparison]::OrdinalIgnoreCase) `
        -and (Split-Path -Leaf $resolvedStage).StartsWith('lychee-compat-')) {
        Remove-Item -LiteralPath $resolvedStage -Recurse -Force -ErrorAction SilentlyContinue
    }
}

$errorCount = @($findings | Where-Object severity -eq 'error').Count
$warningCount = @($findings | Where-Object severity -eq 'warning').Count
$report = [ordered]@{
    schema = $auditSchema
    target = $targetRoot
    policy = if ($useProjectPolicy) { 'lychee-dev' } else { 'generic-addon' }
    generatedAt = (Get-Date).ToUniversalTime().ToString('o')
    method = @(
        'Resolve the actual per-client TOC and recursive XML/Lua load closure.',
        'Validate each isolated closure against one exact official wow-ui-source tag.',
        'Compare manifests, shared load order, client profiles, and generated event catalogs.',
        'Check literal event registrations against the matching exact-build catalog.',
        'Flag dynamic event registrations and compatibility branches that require manual review.'
    )
    clients = @($clientResults)
    findings = @($findings)
    limitations = @(
        'Dynamic API names, dynamically constructed event names, and runtime-only code paths cannot be proven statically.',
        'A clean result does not replace in-game combat, taint, persistence, or interaction testing.',
        'The audit reports compatibility evidence; it does not rewrite target addon source.'
    )
    summary = [ordered]@{
        passed = $errorCount -eq 0
        errors = $errorCount
        warnings = $warningCount
        info = @($findings | Where-Object severity -eq 'info').Count
    }
}

$json = $report | ConvertTo-Json -Depth 12
if ($OutputPath) {
    $resolvedOutput = [System.IO.Path]::GetFullPath((Join-Path (Get-Location) $OutputPath))
    $outputDirectory = Split-Path -Parent $resolvedOutput
    if (-not (Test-Path -LiteralPath $outputDirectory)) {
        New-Item -ItemType Directory -Path $outputDirectory -Force | Out-Null
    }
    [System.IO.File]::WriteAllText($resolvedOutput, $json, [System.Text.UTF8Encoding]::new($false))
}
$json
if ($errorCount -gt 0) {
    exit 1
}
