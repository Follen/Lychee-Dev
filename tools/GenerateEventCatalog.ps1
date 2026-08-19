param(
    [string] $WowdocRepository = "$env:USERPROFILE\.wowdoc\repositories\wow-ui-source.git",
    [ValidateSet("all", "retail", "classic", "titan")]
    [string] $Product = "all",
    [string] $Commit,
    [string] $Version,
    [string] $OutputPath
)

$ErrorActionPreference = "Stop"

$builds = @(
    [pscustomobject]@{
        Product = "retail"
        Label = "Retail"
        Version = "12.1.0"
        Commit = "31c7f7b9cc79e56c986b365c06a6afbcf3c9177b"
        OutputPath = (Join-Path $PSScriptRoot "..\Modules\Events\CatalogData_Mainline.lua")
    },
    [pscustomobject]@{
        Product = "classic"
        Label = "Classic"
        Version = "5.5.4"
        Commit = "1028c1e687f721ba9d3af14d1b12a5745e4227c7"
        OutputPath = (Join-Path $PSScriptRoot "..\Modules\Events\CatalogData_Mists.lua")
    },
    [pscustomobject]@{
        Product = "titan"
        Label = "Classic Titan"
        Version = "3.80.2"
        Commit = "825d29d3662b372f0bead725ee6abd339e4a77b5"
        OutputPath = (Join-Path $PSScriptRoot "..\Modules\Events\CatalogData_Titan.lua")
    }
)

if ($Product -eq "all" -and ($Commit -or $Version -or $OutputPath)) {
    throw "Commit, Version, and OutputPath overrides require a specific Product."
}

if ($Product -ne "all") {
    $builds = @($builds | Where-Object { $_.Product -eq $Product })
    if ($Commit) {
        $builds[0].Commit = $Commit
    }
    if ($Version) {
        $builds[0].Version = $Version
    }
    if ($OutputPath) {
        $builds[0].OutputPath = $OutputPath
    }
}

$sourcePrefix = "Interface/AddOns/Blizzard_APIDocumentationGenerated"
$entryPattern = [regex]::new('(?ms)^\t\t\{\r?\n(?<body>.*?^\t\t\},)')
$eventPattern = [regex]::new('(?m)^\t\t\tType = "Event",\r?$')
$literalPattern = [regex]::new('(?m)^\t\t\tLiteralName = "(?<name>[A-Z0-9_]+)",\r?$')
$payloadPattern = [regex]::new('(?ms)^\t\t\tPayload =\r?\n\t\t\t\{\r?\n(?<payload>.*?)^\t\t\t\},')
$argumentPattern = [regex]::new('(?m)^\t\t\t\t\{ Name = "(?<name>[^"]+)", Type = "[^"]+".*?\},\r?$')

foreach ($build in $builds) {
    $paths = @(git --git-dir=$WowdocRepository ls-tree -r --name-only $build.Commit -- $sourcePrefix |
        Where-Object { $_ -like "*Documentation.lua" })

    if ($LASTEXITCODE -ne 0 -or $paths.Count -eq 0) {
        throw "Could not read generated documentation from $WowdocRepository at $($build.Commit)."
    }

    $events = @{}
    $eventBlockCount = 0

    foreach ($path in $paths) {
        $source = (git --git-dir=$WowdocRepository show "$($build.Commit):$path") -join "`n"
        if ($LASTEXITCODE -ne 0) {
            throw "Could not read $path at $($build.Commit)."
        }

        foreach ($entry in $entryPattern.Matches($source)) {
            $body = $entry.Groups['body'].Value
            if (-not $eventPattern.IsMatch($body)) {
                continue
            }

            $eventBlockCount++
            $literalMatch = $literalPattern.Match($body)
            if (-not $literalMatch.Success) {
                throw "Event without LiteralName in $path."
            }

            $literalName = $literalMatch.Groups['name'].Value
            if ($events.ContainsKey($literalName)) {
                throw "Duplicate event LiteralName: $literalName."
            }

            $arguments = [System.Collections.Generic.List[string]]::new()
            $payloadMatch = $payloadPattern.Match($body)
            if ($payloadMatch.Success) {
                $payload = $payloadMatch.Groups['payload'].Value
                foreach ($argument in $argumentPattern.Matches($payload)) {
                    $arguments.Add($argument.Groups['name'].Value)
                }

                $declaredArguments = ([regex]::Matches($payload, '(?m)^\t\t\t\t\{ Name = ')).Count
                if ($arguments.Count -ne $declaredArguments) {
                    throw "Could not parse every Payload argument for $literalName."
                }
            }

            $events[$literalName] = $arguments -join ", "
        }
    }

    if ($events.Count -ne $eventBlockCount) {
        throw "Parsed $($events.Count) unique events from $eventBlockCount event blocks."
    }

    $lines = [System.Collections.Generic.List[string]]::new()
    $lines.Add("local ADDON_NAME, ns = ...")
    $lines.Add("")
    $lines.Add("-- Generated from official $($build.Label) $($build.Version) Blizzard_APIDocumentationGenerated.")
    $lines.Add("-- wow-ui-source/$($build.Product) commit: $($build.Commit).")
    $lines.Add("ns.EventCatalogData = {")
    $eventNames = [string[]] $events.Keys
    [System.Array]::Sort($eventNames, [System.StringComparer]::Ordinal)
    foreach ($eventName in $eventNames) {
        $signature = $events[$eventName]
        $lines.Add(('    "{0}", "{1}",' -f $eventName, $signature))
    }
    $lines.Add("}")
    $lines.Add("")

    $resolvedOutputPath = [System.IO.Path]::GetFullPath($build.OutputPath)
    [System.IO.File]::WriteAllLines(
        $resolvedOutputPath,
        $lines,
        [System.Text.UTF8Encoding]::new($false)
    )

    Write-Output "Generated $($events.Count) $($build.Product) events at $resolvedOutputPath."
}
