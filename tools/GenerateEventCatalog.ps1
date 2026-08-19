param(
    [string] $WowdocRepository = "$env:USERPROFILE\.wowdoc\repositories\wow-ui-source.git",
    [string] $Commit = "31c7f7b9cc79e56c986b365c06a6afbcf3c9177b",
    [string] $OutputPath = (Join-Path $PSScriptRoot "..\EventCatalogData.lua")
)

$ErrorActionPreference = "Stop"

$sourcePrefix = "Interface/AddOns/Blizzard_APIDocumentationGenerated"
$paths = git --git-dir=$WowdocRepository ls-tree -r --name-only $Commit -- $sourcePrefix |
    Where-Object { $_ -like "*Documentation.lua" }

if ($LASTEXITCODE -ne 0 -or $paths.Count -eq 0) {
    throw "Could not read generated documentation from $WowdocRepository at $Commit."
}

$entryPattern = [regex]::new('(?ms)^\t\t\{\r?\n(?<body>.*?^\t\t\},)')
$eventPattern = [regex]::new('(?m)^\t\t\tType = "Event",\r?$')
$literalPattern = [regex]::new('(?m)^\t\t\tLiteralName = "(?<name>[A-Z0-9_]+)",\r?$')
$payloadPattern = [regex]::new('(?ms)^\t\t\tPayload =\r?\n\t\t\t\{\r?\n(?<payload>.*?)^\t\t\t\},')
$argumentPattern = [regex]::new('(?m)^\t\t\t\t\{ Name = "(?<name>[^"]+)", Type = "[^"]+".*?\},\r?$')
$events = @{}
$eventBlockCount = 0

foreach ($path in $paths) {
    $source = (git --git-dir=$WowdocRepository show "${Commit}:$path") -join "`n"
    if ($LASTEXITCODE -ne 0) {
        throw "Could not read $path at $Commit."
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
$lines.Add("-- Generated from official Retail 12.1.0 Blizzard_APIDocumentationGenerated.")
$lines.Add("-- Source commit: $Commit")
$lines.Add("ns.EventCatalogData = {")
$eventNames = [string[]] $events.Keys
[System.Array]::Sort($eventNames, [System.StringComparer]::Ordinal)
foreach ($eventName in $eventNames) {
    $signature = $events[$eventName]
    $lines.Add(('    "{0}", "{1}",' -f $eventName, $signature))
}
$lines.Add("}")
$lines.Add("")

[System.IO.File]::WriteAllLines(
    [System.IO.Path]::GetFullPath($OutputPath),
    $lines,
    [System.Text.UTF8Encoding]::new($false)
)

Write-Output "Generated $($events.Count) events at $OutputPath."
