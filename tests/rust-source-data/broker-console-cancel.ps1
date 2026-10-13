param([Parameter(Mandatory=$true)][string]$ReleaseRoot,
      [Parameter(Mandatory=$true)][string]$WorkspaceRoot,
      [Parameter(Mandatory=$true)][string]$Pin,
      [Parameter(Mandatory=$true)][string]$Output)
$ErrorActionPreference='Stop'
$taskRelease=(Resolve-Path -LiteralPath $ReleaseRoot).Path
$taskBinary=(Resolve-Path -LiteralPath (Join-Path $taskRelease 'native/windows-amd64/lycheedev.exe')).Path
$taskHome=(Resolve-Path -LiteralPath $WorkspaceRoot).Path
$taskOutput=[IO.Path]::GetFullPath($Output)
if(Test-Path -LiteralPath $taskOutput){throw 'Use a fresh task-owned report directory.'}
[IO.Directory]::CreateDirectory($taskOutput)|Out-Null
$beforeBinary=(Get-FileHash -LiteralPath $taskBinary -Algorithm SHA256).Hash.ToLowerInvariant()
$beforeRelease=(Get-FileHash -LiteralPath (Join-Path $taskRelease 'release.json') -Algorithm SHA256).Hash.ToLowerInvariant()
# A private hidden console isolates the real CTRL_C event from every other app.
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class ToolkitConsoleFixture {
 [StructLayout(LayoutKind.Sequential)] public struct SA {public int nLength;public IntPtr lpSecurityDescriptor;public int inherit;}
 [StructLayout(LayoutKind.Sequential,CharSet=CharSet.Unicode)] public struct SI {public int cb;public string reserved,desktop,title;public uint x,y,xSize,ySize,xChars,yChars,fill,flags;public short show,reserved2;public IntPtr reservedPtr,input,output,error;}
 [StructLayout(LayoutKind.Sequential)] public struct PI {public IntPtr process,thread;public uint pid,tid;}
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool CreateProcessW(string app,string line,IntPtr p,IntPtr t,bool inherit,uint flags,IntPtr env,string cwd,ref SI start,out PI info);
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern IntPtr CreateFileW(string path,uint access,uint share,ref SA sa,uint creation,uint attributes,IntPtr template);
 [DllImport("kernel32.dll",SetLastError=true)] public static extern bool CloseHandle(IntPtr handle);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool AttachConsole(uint pid);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool FreeConsole();
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool SetConsoleCtrlHandler(IntPtr handler,bool add);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool GenerateConsoleCtrlEvent(uint kind,uint group);
 [DllImport("kernel32.dll",SetLastError=true)] public static extern uint WaitForSingleObject(IntPtr handle,uint ms);
 [DllImport("kernel32.dll",SetLastError=true)] public static extern bool GetExitCodeProcess(IntPtr process,out uint code);
 [DllImport("kernel32.dll",SetLastError=true)] public static extern bool TerminateProcess(IntPtr process,uint exit);
 public static PI Start(string binary,string args,string cwd,string stdout,string stderr) {
  SA sa=new SA{nLength=Marshal.SizeOf(typeof(SA)),inherit=1};
  IntPtr output=CreateFileW(stdout,0x40000000,1,ref sa,1,0x80,IntPtr.Zero),error=CreateFileW(stderr,0x40000000,1,ref sa,1,0x80,IntPtr.Zero),input=CreateFileW("NUL",0x80000000,3,ref sa,3,0x80,IntPtr.Zero);
  if(output==new IntPtr(-1)||error==new IntPtr(-1)||input==new IntPtr(-1))throw new Exception("fixture.output:"+Marshal.GetLastWin32Error());
  SI si=new SI{cb=Marshal.SizeOf(typeof(SI)),flags=0x101,show=0,input=input,output=output,error=error}; PI info;
  bool ok=CreateProcessW(binary,"\""+binary+"\" "+args,IntPtr.Zero,IntPtr.Zero,true,0x10,IntPtr.Zero,cwd,ref si,out info);
  CloseHandle(input);CloseHandle(output);CloseHandle(error);
  if(!ok)throw new Exception("fixture.spawn:"+Marshal.GetLastWin32Error());CloseHandle(info.thread);return info;
 }
 public static void CtrlC(uint pid) {
  FreeConsole(); if(!AttachConsole(pid))throw new Exception("fixture.attach:"+Marshal.GetLastWin32Error());
  try {if(!SetConsoleCtrlHandler(IntPtr.Zero,true)||!GenerateConsoleCtrlEvent(0,0))throw new Exception("fixture.signal:"+Marshal.GetLastWin32Error());}
  finally {FreeConsole();}
 }
}
'@

if($Pin -notmatch '^PIN-[a-f0-9]{64}$'){throw 'Exact pin required.'}
foreach($path in @($taskBinary,$taskHome,$taskRelease)){if($path.Contains('"')){throw 'Unexpected quote in fixture path.'}}
$initial=& $taskBinary source session status --release $taskRelease --home $taskHome --format json | ConvertFrom-Json
if($LASTEXITCODE -ne 0 -or $initial.result.state -ne 'closed'){throw 'Fixture requires absent broker.'}
$arguments='source refs --snapshot '+$Pin+' --symbol First --direction both --session-reuse --release "'+$taskRelease+'" --home "'+$taskHome+'" --format json'
$info=[ToolkitConsoleFixture]::Start($taskBinary,$arguments,(Get-Location).Path,(Join-Path $taskOutput 'stdout.json'),(Join-Path $taskOutput 'stderr.txt'))
$children=@();$heldChild=$null;$childCreated=$null;$childExit=$null;$signal=$false;$forced=$false;$failure=$null;$elapsed=$null;[uint32]$code=259
try {
 $observationLimit=[DateTime]::UtcNow.AddSeconds(10)
 do {
  # No command line is retained: hidden startup argv contains a one-use token.
  $children=@(Get-CimInstance Win32_Process -Filter ('ParentProcessId='+$info.pid) | Where-Object Name -eq 'lycheedev.exe' | Select-Object ProcessId,CreationDate,Name)
  if($children.Count -gt 0){break}
  if([ToolkitConsoleFixture]::WaitForSingleObject($info.process,0) -eq 0){break}
 } while([DateTime]::UtcNow -lt $observationLimit)
 if($children.Count -ne 1){throw 'No single owned startup broker observed; startup cancellation is not established.'}
 $heldChild=[System.Diagnostics.Process]::GetProcessById($children[0].ProcessId)
 $handle=$heldChild.Handle
 $childCreated=[string]$heldChild.StartTime.ToUniversalTime().ToFileTimeUtc()
 $timer=[System.Diagnostics.Stopwatch]::StartNew()
 [ToolkitConsoleFixture]::CtrlC($info.pid);$signal=$true
 if([ToolkitConsoleFixture]::WaitForSingleObject($info.process,10000) -ne 0){$forced=$true;[ToolkitConsoleFixture]::TerminateProcess($info.process,8)|Out-Null;[ToolkitConsoleFixture]::WaitForSingleObject($info.process,5000)|Out-Null}
 $elapsed=$timer.Elapsed.TotalMilliseconds
 [ToolkitConsoleFixture]::GetExitCodeProcess($info.process,[ref]$code)|Out-Null
 if(-not $heldChild.WaitForExit(5000)){throw 'Original startup child did not terminate and reap.'}
 $childExit=$heldChild.ExitCode
} catch {$failure=$_.Exception.Message} finally {
 if([ToolkitConsoleFixture]::WaitForSingleObject($info.process,0) -ne 0){$forced=$true;[ToolkitConsoleFixture]::TerminateProcess($info.process,8)|Out-Null;[ToolkitConsoleFixture]::WaitForSingleObject($info.process,5000)|Out-Null}
 [ToolkitConsoleFixture]::CloseHandle($info.process)|Out-Null
 if($heldChild){$heldChild.Dispose()}
 $envelope=$null;try{$envelope=Get-Content -LiteralPath (Join-Path $taskOutput 'stdout.json') -Raw | ConvertFrom-Json}catch{}
 $launch=$null;try{$launch=Get-Content -LiteralPath (Join-Path $taskHome 'tmp/source-broker-launch-last.json') -Raw | ConvertFrom-Json}catch{}
 # Authenticated owner close is cleanup; no process ID is externally terminated.
 $close=& $taskBinary source session close --release $taskRelease --home $taskHome --format json | ConvertFrom-Json
 $closeCode=$LASTEXITCODE
 $survivors=@($children | Where-Object {Get-Process -Id $_.ProcessId -ErrorAction SilentlyContinue})
 $afterBinary=(Get-FileHash -LiteralPath $taskBinary -Algorithm SHA256).Hash.ToLowerInvariant()
 $afterRelease=(Get-FileHash -LiteralPath (Join-Path $taskRelease 'release.json') -Algorithm SHA256).Hash.ToLowerInvariant()
 $startupCancelled=$launch.parentPid -eq $info.pid -and $launch.complete -eq $false -and $launch.child.pid -eq $children[0].ProcessId -and $launch.child.created -eq $childCreated
 $passed=$signal -and !$forced -and !$failure -and $code -eq 7 -and $elapsed -lt 2000 -and $survivors.Count -eq 0 -and $startupCancelled -and $envelope.error.code -eq 'command.cancelled' -and $closeCode -eq 0 -and $close.result.state -eq 'closed' -and $beforeBinary -eq $afterBinary -and $beforeRelease -eq $afterRelease
 $report=@{schema='lycheedev.source-broker-console-cancel.v1';passed=$passed;binary=$taskBinary;release=$taskRelease;before=@{binary=$beforeBinary;release=$beforeRelease};after=@{binary=$afterBinary;release=$afterRelease};home=$taskHome;pin=$Pin;publicProcessId=$info.pid;observedChildren=$children;realCtrlC=$signal;forcedFixtureCleanup=$forced;exitCode=$code;cancelToParentExitMs=$elapsed;originalChildCreated=$childCreated;originalChildExitCode=$childExit;survivors=$survivors;envelope=$envelope;launchDiagnostic=$launch;authenticatedClose=$close;failure=$failure;scope='actual hidden private-console CTRL_C while the exact owned startup child is alive, same caller token, original child kernel handle exit/reap; excludes active retained-request sibling and foreign/replay matrices'}
 [IO.File]::WriteAllText((Join-Path $taskOutput 'report.json'),($report|ConvertTo-Json -Depth 15))
}
if(-not $report.passed){throw 'Public startup cancellation proof did not pass.'}
