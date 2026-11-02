$ErrorActionPreference = 'Stop'
[Console]::InputEncoding = [Text.UTF8Encoding]::new($false)
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
$request = [Console]::In.ReadToEnd() | ConvertFrom-Json
Import-Module Microsoft.PowerShell.Security -ErrorAction Stop
$candidates = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
foreach ($hive in @([Microsoft.Win32.RegistryHive]::CurrentUser, [Microsoft.Win32.RegistryHive]::LocalMachine)) {
    foreach ($view in @([Microsoft.Win32.RegistryView]::Registry64, [Microsoft.Win32.RegistryView]::Registry32)) {
        $base = $null
        $uninstall = $null
        try {
            $base = [Microsoft.Win32.RegistryKey]::OpenBaseKey($hive, $view)
            $uninstall = $base.OpenSubKey('Software\Microsoft\Windows\CurrentVersion\Uninstall')
            if ($null -eq $uninstall) { continue }
            foreach ($name in $uninstall.GetSubKeyNames()) {
                $key = $null
                try {
                    $key = $uninstall.OpenSubKey($name)
                    $display = [string]$key.GetValue('DisplayName')
                    if (-not $display.StartsWith($request.name_prefix, [StringComparison]::OrdinalIgnoreCase)) { continue }
                    foreach ($field in @('InstallLocation', 'DisplayIcon', 'UninstallString')) {
                        $value = ([string]$key.GetValue($field)).Trim()
                        if (-not $value) { continue }
                        if ($field -eq 'InstallLocation') { $directory = $value.Trim('"') }
                        else {
                            $match = [regex]::Match($value, '^"([^"]+)"|^(.+?\.exe)(?:[\s,]|$)', [Text.RegularExpressions.RegexOptions]::IgnoreCase)
                            if (-not $match.Success) { continue }
                            $executable = if ($match.Groups[1].Success) { $match.Groups[1].Value } else { $match.Groups[2].Value }
                            $directory = [IO.Path]::GetDirectoryName($executable)
                        }
                        if ($directory) { [void]$candidates.Add([IO.Path]::GetFullPath($directory)) }
                    }
                } catch { } finally { if ($null -ne $key) { $key.Dispose() } }
            }
        } catch { } finally {
            if ($null -ne $uninstall) { $uninstall.Dispose() }
            if ($null -ne $base) { $base.Dispose() }
        }
    }
}
foreach ($root in ($candidates | Sort-Object)) {
    try {
        $directory = $root
        if ($request.program_subdirectory -and [IO.Path]::GetFileName($root.TrimEnd('\')) -ine $request.program_subdirectory) {
            $directory = Join-Path $root $request.program_subdirectory
        }
        $executable = Join-Path $directory $request.executable
        $resource = Join-Path $directory $request.resource
        if (-not [IO.File]::Exists($executable) -or -not [IO.File]::Exists($resource)) { continue }
        $signature = Get-AuthenticodeSignature -LiteralPath $executable
        if ([string]$signature.Status -ne 'Valid' -or $null -eq $signature.SignerCertificate) { continue }
        $organization = [regex]::Match($signature.SignerCertificate.Subject, '(?:^|,\s*)O=(?:"([^"]+)"|([^,]+))', [Text.RegularExpressions.RegexOptions]::IgnoreCase)
        if (-not $organization.Success) { continue }
        $publisher = if ($organization.Groups[1].Success) { $organization.Groups[1].Value.Trim() } else { $organization.Groups[2].Value.Trim() }
        if (-not ($request.publishers -icontains $publisher)) { continue }
        $version = [Diagnostics.FileVersionInfo]::GetVersionInfo($executable).FileVersion
        [Console]::Out.Write(([ordered]@{ directory = $directory; executable = $executable; resource = $resource; publisher = $publisher; version = $version } | ConvertTo-Json -Compress))
        exit 0
    } catch { }
}
[Console]::Out.Write('{}')
