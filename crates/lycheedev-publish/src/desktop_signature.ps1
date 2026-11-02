$ErrorActionPreference = 'Stop'
[Console]::InputEncoding = [Text.UTF8Encoding]::new($false)
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
$request = [Console]::In.ReadToEnd() | ConvertFrom-Json
Import-Module Microsoft.PowerShell.Security -ErrorAction Stop
if (-not [IO.File]::Exists($request.path)) { exit 1 }
$signature = Get-AuthenticodeSignature -LiteralPath $request.path
if ([string]$signature.Status -ne 'Valid' -or $null -eq $signature.SignerCertificate) { exit 1 }
$organization = [regex]::Match($signature.SignerCertificate.Subject, '(?:^|,\s*)O=(?:"([^"]+)"|([^,]+))', [Text.RegularExpressions.RegexOptions]::IgnoreCase)
if (-not $organization.Success) { exit 1 }
$publisher = if ($organization.Groups[1].Success) { $organization.Groups[1].Value.Trim() } else { $organization.Groups[2].Value.Trim() }
if (-not ($request.publishers -icontains $publisher)) { exit 1 }
$version = [Diagnostics.FileVersionInfo]::GetVersionInfo($request.path).FileVersion
[Console]::Out.Write(([ordered]@{path=[IO.Path]::GetFullPath($request.path);publisher=$publisher;version=[string]$version} | ConvertTo-Json -Compress))
