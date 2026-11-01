$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
try {
    Import-Module CimCmdlets -ErrorAction Stop
    $serials = @(Get-CimInstance -ClassName Win32_PhysicalMedia -Property SerialNumber -ErrorAction Stop | ForEach-Object { [string]$_.SerialNumber })
    [Console]::Out.Write((ConvertTo-Json -InputObject $serials -Compress))
} catch {
    # Hardware discovery is optional in the original device-ID generation.
    # Do not emit WMI exception text or identifiers into diagnostic stderr.
    [Console]::Out.Write('null')
}
