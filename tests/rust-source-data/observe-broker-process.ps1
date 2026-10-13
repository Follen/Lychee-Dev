param([Parameter(Mandatory=$true)][int]$TargetPid,
      [Parameter(Mandatory=$true)][string]$ExpectedCreated,
      [int]$WaitMilliseconds=120000)
$ErrorActionPreference='Stop'
if($WaitMilliseconds -lt 1 -or $WaitMilliseconds -gt 180000){throw 'bounded wait required'}
# Test-only observer: hold the original kernel handle before the next request.
# Never kill a process or reopen a recycled PID after the readiness message.
$observed=[System.Diagnostics.Process]::GetProcessById($TargetPid)
try {
    $held=$observed.Handle
    $created=[string]$observed.StartTime.ToUniversalTime().ToFileTimeUtc()
    if($created -ne $ExpectedCreated){throw 'broker creation identity differs'}
    @{phase='ready';pid=$TargetPid;created=$created;privateBytes=$observed.PrivateMemorySize64;workingSetBytes=$observed.WorkingSet64} | ConvertTo-Json -Compress
    [Console]::Out.Flush()
    if(-not $observed.WaitForExit($WaitMilliseconds)){throw 'original broker process wait deadline'}
    @{phase='exited';pid=$TargetPid;created=$created;exitCode=$observed.ExitCode} | ConvertTo-Json -Compress
} finally { $observed.Dispose() }
