$ErrorActionPreference = 'Stop'

$testRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
$previousTestClient = $env:LYCHEE_TEST_CLIENT
Push-Location -LiteralPath $testRoot
try {
    $clients = @('retail', 'classic', 'titan', 'forever')
    $tests = @(
        'tests/CoreTests.lua',
        'tests/LocaleTests.lua',
        'tests/AutomationTests.lua',
        'tests/ReloadTests.lua',
        'tests/EventCatalogTests.lua',
        'tests/EventMonitorTests.lua',
        'tests/DeveloperToolsTests.lua',
        'tests/UITests.lua'
    )

    foreach ($client in $clients) {
        $env:LYCHEE_TEST_CLIENT = $client
        foreach ($test in $tests) {
            & lua $test
            if ($LASTEXITCODE -ne 0) {
                throw "$client failed $test"
            }
        }
    }

    Remove-Item Env:LYCHEE_TEST_CLIENT -ErrorAction SilentlyContinue
    & lua 'tests/BuildMatrixTests.lua'
    if ($LASTEXITCODE -ne 0) {
        throw 'build matrix tests failed'
    }

    & powershell -ExecutionPolicy Bypass -File 'tests/StaticCompatibilityAuditTests.ps1'
    if ($LASTEXITCODE -ne 0) {
        throw 'static compatibility audit tests failed'
    }

    & powershell -NoProfile -ExecutionPolicy Bypass -File 'tests/PackagingTests.ps1'
    if ($LASTEXITCODE -ne 0) {
        throw 'packaging tests failed'
    }

} finally {
    if ($null -eq $previousTestClient) {
        Remove-Item Env:LYCHEE_TEST_CLIENT -ErrorAction SilentlyContinue
    } else {
        $env:LYCHEE_TEST_CLIENT = $previousTestClient
    }
    Pop-Location
}
