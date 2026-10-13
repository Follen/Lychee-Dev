param([Parameter(Mandatory=$true)][string]$ReleaseRoot,
      [Parameter(Mandatory=$true)][string]$WorkspaceRoot,
      [Parameter(Mandatory=$true)][string]$Pin,
      [Parameter(Mandatory=$true)][string]$Output)
$ErrorActionPreference='Stop'
Import-Module (Join-Path $PSHOME 'Modules/CimCmdlets/CimCmdlets.psd1')
function FileHash([string]$Path) {
 $stream=[IO.File]::OpenRead($Path);$algorithm=[Security.Cryptography.SHA256]::Create()
 try{return ([BitConverter]::ToString($algorithm.ComputeHash($stream))).Replace('-','').ToLowerInvariant()}finally{$algorithm.Dispose();$stream.Dispose()}
}
$taskRelease=(Resolve-Path -LiteralPath $ReleaseRoot).Path
$taskBinary=Join-Path $taskRelease 'native/windows-amd64/lycheedev.exe'
$taskWorker=Join-Path $taskRelease 'payload/tool/luals/bin/lua-language-server.exe'
$taskHome=(Resolve-Path -LiteralPath $WorkspaceRoot).Path
$taskOutput=[IO.Path]::GetFullPath($Output)
if(Test-Path -LiteralPath $taskOutput){throw 'Fresh fixture output required'}
[IO.Directory]::CreateDirectory($taskOutput)|Out-Null
if($Pin -notmatch '^PIN-[a-f0-9]{64}$'){throw 'Exact pin required'}
foreach($p in @($taskBinary,$taskHome,$taskRelease)){if($p.Contains('"')){throw 'Unexpected path quote'}}
$binaryHash=FileHash $taskBinary
$workerHash=FileHash $taskWorker
# Reuse only the existing private-console native helper, not its startup test.
$helperSource=[IO.File]::ReadAllText((Join-Path $PSScriptRoot 'broker-console-cancel.ps1'))
$helper=[regex]::Match($helperSource,"Add-Type -TypeDefinition @'\r?\n([\s\S]*?)\r?\n'@")
if(-not $helper.Success -or $helper.Groups[1].Value.Length -gt 16384){throw 'Bounded console helper absent'}
Add-Type -TypeDefinition $helper.Groups[1].Value
function JsonRead([string]$Path,[int]$Max) {
 $s=[IO.File]::OpenRead($Path)
 try {if($s.Length -gt $Max){throw 'JSON input budget'};$reader=[IO.StreamReader]::new($s);try{return $reader.ReadToEnd()|ConvertFrom-Json}finally{$reader.Dispose()}}finally{$s.Dispose()}
}
function Call([string]$Name,[string[]]$CommandTokens) {
 $raw=& $taskBinary @CommandTokens --release $taskRelease --home $taskHome --format json
 $exit=$LASTEXITCODE
 [IO.File]::WriteAllText((Join-Path $taskOutput ($Name+'.json')),($raw -join "`n"))
 if($exit -ne 0){throw ($Name+' failed with exit '+$exit)}
 return ($raw -join "`n"|ConvertFrom-Json)
}
$initial=Call 'absent' @('source','session','status')
if($initial.result.state -ne 'closed'){throw 'Absent broker required'}
$arguments='source refs --snapshot '+$Pin+' --symbol CallbackRegistryMixin:RegisterCallback --direction incoming --limit 10 --session-reuse --release "'+$taskRelease+'" --home "'+$taskHome+'" --format json'
$info=[ToolkitConsoleFixture]::Start($taskBinary,$arguments,(Get-Location).Path,(Join-Path $taskOutput 'query.stdout.json'),(Join-Path $taskOutput 'query.stderr.txt'))
$heldBroker=$null;$heldWorker=$null;$identities=@();$signal=$false;$forced=$false;$failure=$null;$brokerExit=$null;$workerExit=$null;$close=$null;$status=$null;[uint32]$exitCode=259
try {
 $until=[DateTime]::UtcNow.AddSeconds(60)
 do {
  if([ToolkitConsoleFixture]::WaitForSingleObject($info.process,0) -eq 0){throw 'Query exited before live worker observation'}
  $brokers=@(Get-CimInstance Win32_Process -Filter ('ParentProcessId='+$info.pid)|Where-Object Name -eq 'lycheedev.exe')
  if($brokers.Count -gt 1){throw 'Ambiguous broker child'}
  if(-not $heldBroker -and $brokers.Count -eq 1){
   $heldBroker=[Diagnostics.Process]::GetProcessById($brokers[0].ProcessId);$null=$heldBroker.Handle
   $created=[string]$heldBroker.StartTime.ToUniversalTime().ToFileTimeUtc()
   if((FileHash $heldBroker.MainModule.FileName) -ne $binaryHash){throw 'Broker image differs'}
   $identities+=@{kind='broker';pid=$heldBroker.Id;created=$created;image=$heldBroker.MainModule.FileName;sha256=$binaryHash}
  }
  if($heldBroker){
   if($heldBroker.HasExited){throw 'Held broker exited before worker observation'}
   $workers=@(Get-CimInstance Win32_Process -Filter ('ParentProcessId='+$heldBroker.Id)|Where-Object Name -eq 'lua-language-server.exe')
   if($workers.Count -gt 1){throw 'Ambiguous semantic worker'}
   if($workers.Count -eq 1){
    $heldWorker=[Diagnostics.Process]::GetProcessById($workers[0].ProcessId);$null=$heldWorker.Handle
    $created=[string]$heldWorker.StartTime.ToUniversalTime().ToFileTimeUtc()
    if([long]$created -lt [long]$identities[0].created -or (FileHash $heldWorker.MainModule.FileName) -ne $workerHash){throw 'Worker creation/image differs'}
    $record=JsonRead (Join-Path $taskHome 'tmp/source-broker-last-run.json') 4096
    if($record.schema -ne 'lycheedev.source-broker-lifecycle.v1' -or $record.pid -ne $heldBroker.Id -or $record.created -isnot [string] -or $record.created -ne $identities[0].created -or $record.terminal -ne $false){throw 'Lifecycle does not bind held broker'}
    $identities+=@{kind='worker';pid=$heldWorker.Id;created=$created;image=$heldWorker.MainModule.FileName;sha256=$workerHash;parentPid=$heldBroker.Id}
    break
   }
  }
  Start-Sleep -Milliseconds 25
 }while([DateTime]::UtcNow -lt $until)
 if(-not $heldWorker){throw 'No actual LuaLS worker observed in full-source request'}
 if([ToolkitConsoleFixture]::WaitForSingleObject($info.process,0) -eq 0 -or $heldWorker.HasExited){throw 'Query/worker no longer alive before CtrlC'}
 $signalUnixMs=[DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
 $timer=[Diagnostics.Stopwatch]::StartNew()
 [ToolkitConsoleFixture]::CtrlC($info.pid);$signal=$true
 if([ToolkitConsoleFixture]::WaitForSingleObject($info.process,15000) -ne 0){throw 'Graceful caller cancellation deadline'}
 [ToolkitConsoleFixture]::GetExitCodeProcess($info.process,[ref]$exitCode)|Out-Null
 $cancelMs=$timer.Elapsed.TotalMilliseconds
 if(-not $heldWorker.WaitForExit(15000)){throw 'Original LuaLS worker not reaped after cancellation'}
 $workerExit=$heldWorker.ExitCode
 $workerExitUnixMs=([DateTimeOffset]$heldWorker.ExitTime.ToUniversalTime()).ToUnixTimeMilliseconds()
 $envelope=JsonRead (Join-Path $taskOutput 'query.stdout.json') (16*1024*1024)
 if($exitCode -ne 7 -or $envelope.error.code -ne 'command.cancelled'){throw 'Caller cancellation classification differs'}
 if($workerExit -ne 7 -or $workerExitUnixMs -lt $signalUnixMs){throw 'Held worker retirement is not attributable to cancellation interval'}
 $status=Call 'after-cancel' @('source','session','status')
 if($status.result.state -ne 'idle' -or $status.result.workerPid -ne 0 -or $status.result.batches -ne 0){throw 'Cancelled worker remained reusable'}
}catch{$failure=$_.Exception.Message}finally{
 if([ToolkitConsoleFixture]::WaitForSingleObject($info.process,0) -ne 0){$forced=$true;[ToolkitConsoleFixture]::TerminateProcess($info.process,8)|Out-Null;[ToolkitConsoleFixture]::WaitForSingleObject($info.process,5000)|Out-Null}
 [ToolkitConsoleFixture]::CloseHandle($info.process)|Out-Null
 try {
  $close=Call 'owned-close' @('source','session','close')
  if($close.result.state -ne 'closed'){throw 'Authenticated close not closed'}
  if($heldBroker){if(-not $heldBroker.WaitForExit(5000)){throw 'Original broker did not exit after close'};$brokerExit=$heldBroker.ExitCode}
  if($heldWorker -and -not $heldWorker.WaitForExit(5000)){throw 'Original worker still alive after close'}
 }catch{if(-not $failure){$failure=$_.Exception.Message}}
 $passed=$signal -and -not $forced -and -not $failure -and $exitCode -eq 7 -and $workerExit -eq 7 -and $brokerExit -eq 0
 $report=@{schema='lycheedev.source-active-console-cancel.v1';passed=$passed;binary=$taskBinary;binarySHA256=$binaryHash;workerSHA256=$workerHash;home=$taskHome;pin=$Pin;argv=$arguments;publicPid=$info.pid;identities=$identities;realCtrlC=$signal;forcedFixtureCleanup=$forced;callerExit=$exitCode;workerExit=$workerExit;brokerExit=$brokerExit;signalUnixMs=$signalUnixMs;workerExitUnixMs=$workerExitUnixMs;cancelToCallerExitMs=$cancelMs;envelope=$envelope;statusAfterCancel=$status;authenticatedClose=$close;failure=$failure;scope='full Retail query in flight with actual held broker and LuaLS worker; private console CtrlC, typed cancellation and original worker retirement; does not prove initialization completed or a specific LSP request was executing'}
 [IO.File]::WriteAllText((Join-Path $taskOutput 'report.json'),($report|ConvertTo-Json -Depth 15))
 if($heldWorker){$heldWorker.Dispose()};if($heldBroker){$heldBroker.Dispose()}
}
if(-not $report.passed){throw ('Active worker cancellation failed: '+$report.failure)}
