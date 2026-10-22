param([Parameter(Mandatory=$true)][string]$Binary,[Parameter(Mandatory=$true)][string]$WorkspaceRoot,[string]$Pin,[Parameter(Mandatory=$true)][string]$Output,[string]$CommandArguments,[string]$ChildName='git*.exe')
$ErrorActionPreference='Stop'
$taskBinary=(Resolve-Path -LiteralPath $Binary).Path
$taskHome=(Resolve-Path -LiteralPath $WorkspaceRoot).Path
$taskOutput=[IO.Path]::GetFullPath($Output)
if(Test-Path -LiteralPath $taskOutput){throw 'Use a fresh task-owned report directory.'}
[IO.Directory]::CreateDirectory($taskOutput)|Out-Null
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
if(!$CommandArguments -and $Pin -notmatch '^PIN-[a-f0-9]{64}$'){throw 'Exact pin required.'}
if($taskBinary.Contains('"') -or $taskHome.Contains('"')){throw 'Unexpected quote in fixture paths.'}
$arguments=if($CommandArguments){$CommandArguments}else{'source index --snapshot '+$Pin+' --home "'+$taskHome+'" --format json'}
$info=[ToolkitConsoleFixture]::Start($taskBinary,$arguments,(Get-Location).Path,(Join-Path $taskOutput 'stdout.json'),(Join-Path $taskOutput 'stderr.txt'))
$children=@();$signal=$false;$forced=$false;[uint32]$code=259
try {
 $observationLimit=[DateTime]::UtcNow.AddSeconds(10)
 do {
  $children=@(Get-CimInstance Win32_Process -Filter ('ParentProcessId='+$info.pid) | Where-Object Name -like $ChildName | Select-Object ProcessId,CreationDate,Name,CommandLine)
  if($children.Count -gt 0){break}
  if([ToolkitConsoleFixture]::WaitForSingleObject($info.process,0) -eq 0){break}
 } while([DateTime]::UtcNow -lt $observationLimit)
 if($children.Count -eq 0){throw 'No active owned child observed; cancellation proof is not established.'}
 [ToolkitConsoleFixture]::CtrlC($info.pid);$signal=$true
 if([ToolkitConsoleFixture]::WaitForSingleObject($info.process,10000) -ne 0){$forced=$true;[ToolkitConsoleFixture]::TerminateProcess($info.process,8)|Out-Null;[ToolkitConsoleFixture]::WaitForSingleObject($info.process,5000)|Out-Null}
 [ToolkitConsoleFixture]::GetExitCodeProcess($info.process,[ref]$code)|Out-Null
} finally {
 if([ToolkitConsoleFixture]::WaitForSingleObject($info.process,0) -ne 0){$forced=$true;[ToolkitConsoleFixture]::TerminateProcess($info.process,8)|Out-Null;[ToolkitConsoleFixture]::WaitForSingleObject($info.process,5000)|Out-Null}
 [ToolkitConsoleFixture]::CloseHandle($info.process)|Out-Null
 $survivors=@($children | Where-Object {Get-Process -Id $_.ProcessId -ErrorAction SilentlyContinue})
 $envelope=$null;try{$envelope=Get-Content -LiteralPath (Join-Path $taskOutput 'stdout.json') -Raw | ConvertFrom-Json}catch{}
 $completeReap=$envelope.error.details.terminated -eq $true -and $envelope.error.details.reaped -eq $true -and $envelope.error.details.stdinWriterComplete -eq $true -and $envelope.error.details.outputReadersComplete -eq $true
 $digest=[Security.Cryptography.SHA256]::Create();$file=[IO.File]::OpenRead($taskBinary)
 try{$binarySHA=[BitConverter]::ToString($digest.ComputeHash($file)).Replace('-','').ToLowerInvariant()}finally{$file.Dispose();$digest.Dispose()}
 $report=@{schema='lycheedev.public-console-cancel.v1';binary=@{path=$taskBinary;sha256=$binarySHA};home=$taskHome;pin=$Pin;argv=$arguments;processId=$info.pid;observedChildren=$children;realCtrlC=$signal;forcedFixtureCleanup=$forced;exitCode=$code;survivors=$survivors;envelope=$envelope;state=$(if($signal -and !$forced -and $code -eq 7 -and $survivors.Count -eq 0 -and $envelope.error.code -eq 'command.cancelled' -and $completeReap){'passed'}else{'failed_or_not_run'})}
 [IO.File]::WriteAllText((Join-Path $taskOutput 'report.json'),($report|ConvertTo-Json -Depth 12))
}
if($report.state -ne 'passed'){throw ('Public cancellation proof did not pass: '+$report.state)}
