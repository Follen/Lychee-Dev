$ErrorActionPreference = 'Stop'

$raw = & powershell -ExecutionPolicy Bypass -File 'tools/AuditCompatibility.ps1' -SkipWowdoc
if ($LASTEXITCODE -ne 0) {
    throw 'static compatibility audit failed'
}
$report = ($raw | Out-String) | ConvertFrom-Json
if ($report.schema -ne 'lychee.compatibility-audit.v1' -or -not $report.summary.passed) {
    throw 'static compatibility audit report is invalid'
}
if (@($report.clients).Count -ne 3) {
    throw 'static compatibility audit did not cover all three clients'
}
foreach ($client in @($report.clients)) {
    if ($client.loadClosureFiles -lt 20 -or $client.luaFiles -lt 20) {
        throw "$($client.id) load closure was incomplete"
    }
    if ($client.literalEvents -lt 1) {
        throw "$($client.id) literal event coverage was not recorded"
    }
    if ($null -ne $client.wowdoc) {
        throw "$($client.id) local-only audit unexpectedly ran wowdoc"
    }
}

Write-Output 'Lychee Dev static compatibility audit tests passed'
