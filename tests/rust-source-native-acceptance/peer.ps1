param([Parameter(Mandatory=$true)][string]$Spec)
# This helper is a child of the acceptance controller's owned, non-breakaway
# Job. It never launches/terminates a server and never substitutes Hello for
# kernel peer authentication. Every exchange holds its actual pipe and peer
# process handle until the response (or rejection) has been observed.
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
[Console]::OutputEncoding=New-Object Text.UTF8Encoding($false)
$utf8=New-Object Text.UTF8Encoding($false)
Add-Type -ReferencedAssemblies System.dll,System.Core.dll,System.Runtime.Serialization.dll,System.Xml.dll -TypeDefinition @'
using System;
using System.IO;
using System.IO.Pipes;
using System.Text;
using System.Diagnostics;
using System.ComponentModel;
using System.Security.Cryptography;
using System.Runtime.InteropServices;
using System.Runtime.Serialization;
using System.Runtime.Serialization.Json;
using System.Threading.Tasks;
using Microsoft.Win32.SafeHandles;

public sealed class NativeAcceptancePeer : IDisposable {
 [StructLayout(LayoutKind.Sequential)] struct FT { public uint low,high; }
 [DataContract] sealed class Hello {
  [DataMember(IsRequired=true)] public string wire;
  [DataMember(IsRequired=true)] public string executable;
  [DataMember(IsRequired=true)] public uint pid;
  [DataMember(IsRequired=true)] public ulong created;
 }
 public sealed class Owner {
  public uint pid,exitCode; public string created,image,sha256,exitFileTime;
  public long creationUnixMs,lifeMs; public bool exited,imageQueriedThisObservation;
 }
 public sealed class Exchange {
  public string hello,response; public bool rejected;
  public Owner owner; public long startedElapsedMs,finishedElapsedMs;
  public int requestBytes,responseBytes;
 }
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool GetNamedPipeServerProcessId(IntPtr pipe,out uint pid);
 [DllImport("kernel32.dll",SetLastError=true)] static extern IntPtr OpenProcess(uint access,bool inherit,uint pid);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool GetProcessTimes(IntPtr process,out FT created,out FT exited,out FT kernel,out FT user);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool GetExitCodeProcess(IntPtr process,out uint code);
 [DllImport("kernel32.dll",SetLastError=true,CharSet=CharSet.Unicode)] static extern bool QueryFullProcessImageNameW(IntPtr process,uint flags,StringBuilder image,ref uint size);
 [DllImport("kernel32.dll",SetLastError=true)] static extern uint WaitForSingleObject(IntPtr handle,uint ms);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool CloseHandle(IntPtr handle);
 [DllImport("kernel32.dll")] static extern IntPtr GetCurrentProcess();
 [DllImport("advapi32.dll",SetLastError=true)] static extern bool OpenProcessToken(IntPtr process,uint access,out IntPtr token);
 [DllImport("advapi32.dll",SetLastError=true)] static extern bool GetTokenInformation(IntPtr token,int kind,IntPtr data,uint bytes,out uint needed);
 [DllImport("advapi32.dll",SetLastError=true,CharSet=CharSet.Unicode)] static extern bool ConvertSidToStringSidW(IntPtr sid,out IntPtr text);
 [DllImport("kernel32.dll")] static extern IntPtr LocalFree(IntPtr memory);
 [DllImport("kernel32.dll",SetLastError=true,CharSet=CharSet.Unicode)] static extern SafeFileHandle CreateFileW(string path,uint access,uint share,IntPtr security,uint disposition,uint flags,IntPtr template);
 [DllImport("kernel32.dll",SetLastError=true,CharSet=CharSet.Unicode)] static extern uint GetFinalPathNameByHandleW(SafeFileHandle file,StringBuilder path,uint size,uint flags);
 readonly Stopwatch clock=Stopwatch.StartNew();
 readonly int budgetMs; readonly uint expectedPid;
 readonly FileStream binary; readonly string image,digest;
 IntPtr ownerHandle=IntPtr.Zero; uint ownerPid; ulong ownerCreated;
 long traffic; int exchanges; bool disposed;
 public string canonicalHome,logonSid;
 public Exchange lastExchange;
 public long ElapsedMs { get {return clock.ElapsedMilliseconds;} }
 static Exception Error(string stage){return new Win32Exception(Marshal.GetLastWin32Error(),stage);}
 static ulong Ticks(FT value){return ((ulong)value.high<<32)|value.low;}
 static string FinalPath(SafeFileHandle handle){var path=new StringBuilder(32768);uint size=GetFinalPathNameByHandleW(handle,path,32768,0);if(size==0||size>=32768)throw Error("peer.final_path");return path.ToString();}
 static string Canonical(string path){using(var file=CreateFileW(Path.GetFullPath(path),0,7,IntPtr.Zero,3,0x02000000,IntPtr.Zero)){if(file.IsInvalid)throw Error("peer.path_open");return FinalPath(file);}}
 static string LogonSid(){
  IntPtr token; if(!OpenProcessToken(GetCurrentProcess(),8,out token))throw Error("peer.token_open");
  try {uint needed;GetTokenInformation(token,2,IntPtr.Zero,0,out needed);if(needed==0||needed>131072)throw new Exception("peer.token_budget");
   IntPtr data=Marshal.AllocHGlobal((int)needed);
   try {if(!GetTokenInformation(token,2,data,needed,out needed))throw Error("peer.token_groups");
    uint count=(uint)Marshal.ReadInt32(data);int offset=IntPtr.Size==8?8:4,stride=IntPtr.Size==8?16:8;
    if(count>4096||offset+(long)count*stride>needed)throw new Exception("peer.token_bounds");
    for(int i=0;i<count;i++){IntPtr row=IntPtr.Add(data,offset+i*stride);uint attributes=(uint)Marshal.ReadInt32(row,IntPtr.Size);
     if((attributes&0xc0000000)==0xc0000000){IntPtr text;if(!ConvertSidToStringSidW(Marshal.ReadIntPtr(row),out text))throw Error("peer.token_sid");try{return Marshal.PtrToStringUni(text);}finally{LocalFree(text);}}
    }throw new Exception("peer.logon_sid_missing");
   }finally{Marshal.FreeHGlobal(data);}
  }finally{CloseHandle(token);}
 }
 public NativeAcceptancePeer(string executable,string home,string sha256,int totalMs,uint pid){
  if(totalMs<1000||totalMs>720000)throw new Exception("peer.total_budget");budgetMs=totalMs;expectedPid=pid;
  binary=new FileStream(executable,FileMode.Open,FileAccess.Read,FileShare.Read);
  try {if(binary.Length==0||binary.Length>536870912)throw new Exception("peer.binary_budget");image=FinalPath(binary.SafeFileHandle);
   using(var sha=SHA256.Create())digest=BitConverter.ToString(sha.ComputeHash(binary)).Replace("-","").ToLowerInvariant();
   if(!String.Equals(digest,sha256,StringComparison.Ordinal))throw new Exception("peer.binary_digest");
   canonicalHome=Canonical(home);logonSid=LogonSid();
  }catch{binary.Dispose();throw;}
 }
 public int RemainingMs(){long left=budgetMs-clock.ElapsedMilliseconds;if(left<=0)throw new TimeoutException("peer.total_deadline");return (int)left;}
 int Remaining(long deadline){long left=Math.Min(deadline-clock.ElapsedMilliseconds,RemainingMs());if(left<=0)throw new TimeoutException("peer.exchange_deadline");return (int)left;}
 void Wait(Task task,long deadline){if(!task.Wait(Remaining(deadline)))throw new TimeoutException("peer.io_deadline");}
 byte[] ReadExact(NamedPipeClientStream pipe,int count,long deadline){byte[] bytes=new byte[count];int offset=0;while(offset<count){var task=pipe.ReadAsync(bytes,offset,count-offset);Wait(task,deadline);int n=task.Result;if(n==0)throw new EndOfStreamException("peer.frame_eof");offset+=n;}return bytes;}
 string ReadFrame(NamedPipeClientStream pipe,int cap,long deadline,out int length){byte[] header=ReadExact(pipe,4,deadline);uint n=((uint)header[0]<<24)|((uint)header[1]<<16)|((uint)header[2]<<8)|header[3];if(n==0||n>cap)throw new Exception("peer.frame_budget");length=(int)n;return new UTF8Encoding(false,true).GetString(ReadExact(pipe,(int)n,deadline));}
 void WriteFrame(NamedPipeClientStream pipe,byte[] bytes,long deadline){int n=bytes.Length;byte[] header={(byte)(n>>24),(byte)(n>>16),(byte)(n>>8),(byte)n};Wait(pipe.WriteAsync(header,0,4),deadline);Wait(pipe.WriteAsync(bytes,0,n),deadline);Wait(pipe.FlushAsync(),deadline);}
 Owner Identity(IntPtr handle,uint pid){
  FT created,exit,kernel,user;if(!GetProcessTimes(handle,out created,out exit,out kernel,out user))throw Error("peer.process_times");
  uint wait=WaitForSingleObject(handle,0);if(wait!=0&&wait!=258)throw Error("peer.process_wait");
  if(wait==0&&!GetProcessTimes(handle,out created,out exit,out kernel,out user))throw Error("peer.exited_process_times");
  ulong ticks=Ticks(created);uint code;if(!GetExitCodeProcess(handle,out code))throw Error("peer.exit_code");
  // QueryFullProcessImageName may no longer be available after exit. Only the
  // already authenticated, continuously held owner handle can use its binding.
  bool retained=wait==0&&handle==ownerHandle&&pid==ownerPid&&ticks==ownerCreated;
  string actual=image;
  if(!retained){var name=new StringBuilder(32768);uint count=32768;if(!QueryFullProcessImageNameW(handle,0,name,ref count))throw Error("peer.process_image");
   actual=Canonical(name.ToString());if(!String.Equals(actual,image,StringComparison.OrdinalIgnoreCase))throw new Exception("peer.process_image_mismatch");}
  return new Owner{pid=pid,created=ticks.ToString(),image=actual,sha256=digest,creationUnixMs=(long)(ticks/10000)-11644473600000L,exited=wait==0,exitCode=code,exitFileTime=Ticks(exit).ToString(),lifeMs=wait==0?(long)((Ticks(exit)-ticks)/10000):-1,imageQueriedThisObservation=!retained};
 }
 public Exchange Request(string pipeName,string requestPath,int timeoutMs,bool reject){
  if(++exchanges>850||timeoutMs<1||timeoutMs>30000)throw new Exception("peer.request_budget");
  var file=new FileInfo(requestPath);if(file.Length<1||file.Length>2097152)throw new Exception("peer.request_bytes");
  byte[] bytes=File.ReadAllBytes(requestPath);if(bytes.Length!=file.Length)throw new Exception("peer.request_changed");
  long begun=clock.ElapsedMilliseconds,deadline=begun+Math.Min(timeoutMs,RemainingMs());
  lastExchange=new Exchange{startedElapsedMs=begun,requestBytes=bytes.Length};
  using(var pipe=new NamedPipeClientStream(".",pipeName,PipeDirection.InOut,PipeOptions.Asynchronous,System.Security.Principal.TokenImpersonationLevel.Identification)){
   pipe.Connect(Remaining(deadline));uint pid;if(!GetNamedPipeServerProcessId(pipe.SafePipeHandle.DangerousGetHandle(),out pid))throw Error("peer.pipe_server_pid");
   IntPtr process=OpenProcess(0x00101000,false,pid);if(process==IntPtr.Zero)throw Error("peer.process_open");
   try {var identity=Identity(process,pid);lastExchange.owner=identity;if(identity.exited||(expectedPid!=0&&pid!=expectedPid))throw new Exception("peer.owner_pid_mismatch");
    int helloBytes;string rawHello=ReadFrame(pipe,4096,deadline,out helloBytes);Hello hello;
    lastExchange.hello=rawHello;
    using(var memory=new MemoryStream(Encoding.UTF8.GetBytes(rawHello)))hello=(Hello)new DataContractJsonSerializer(typeof(Hello)).ReadObject(memory);
    if(hello.wire!="lycheedev.native-source-session.v1"||hello.executable!=digest||hello.pid!=pid||hello.created.ToString()!=identity.created)throw new Exception("peer.hello_identity_mismatch");
    uint stillPid;if(!GetNamedPipeServerProcessId(pipe.SafePipeHandle.DangerousGetHandle(),out stillPid)||stillPid!=pid||WaitForSingleObject(process,0)!=258)throw new Exception("peer.identity_changed");
    if(ownerHandle==IntPtr.Zero){ownerHandle=process;process=IntPtr.Zero;ownerPid=pid;ownerCreated=hello.created;}
    else if(ownerPid!=pid||ownerCreated!=hello.created)throw new Exception("peer.owner_generation_changed");
    WriteFrame(pipe,bytes,deadline);string response=null;int responseBytes=0;
    if(reject){var one=new byte[1];var task=pipe.ReadAsync(one,0,1);Wait(task,deadline);if(task.Result!=0)throw new Exception("peer.invalid_envelope_replied");}
    else response=ReadFrame(pipe,2097152,deadline,out responseBytes);
    lastExchange.response=response;lastExchange.responseBytes=responseBytes;lastExchange.rejected=reject;lastExchange.finishedElapsedMs=clock.ElapsedMilliseconds;
    traffic+=bytes.Length+responseBytes+helloBytes+8;if(traffic>67108864)throw new Exception("peer.traffic_budget");
    return lastExchange;
   }catch{lastExchange.finishedElapsedMs=clock.ElapsedMilliseconds;throw;}
   finally{if(process!=IntPtr.Zero&&!CloseHandle(process))throw Error("peer.close_exchange_process");}
  }
 }
 public Owner Observe(){if(ownerHandle==IntPtr.Zero)throw new Exception("peer.owner_unbound");var owner=Identity(ownerHandle,ownerPid);if(owner.created!=ownerCreated.ToString())throw new Exception("peer.owner_creation_changed");return owner;}
 public Owner WaitExit(int timeoutMs){if(timeoutMs<0||timeoutMs>RemainingMs())throw new Exception("peer.wait_budget");if(ownerHandle==IntPtr.Zero)throw new Exception("peer.owner_unbound");uint status=WaitForSingleObject(ownerHandle,(uint)timeoutMs);if(status!=0&&status!=258)throw Error("peer.wait_exit");return Observe();}
 public void Dispose(){if(disposed)return;disposed=true;bool ok=ownerHandle==IntPtr.Zero||CloseHandle(ownerHandle);ownerHandle=IntPtr.Zero;binary.Dispose();if(!ok)throw Error("peer.close_owner_process");}
}
'@
$peer=$null
try {
 if((Get-Item -LiteralPath $Spec).Length -gt 32768){throw 'peer.spec_budget'}
 $settings=[IO.File]::ReadAllText($Spec)|ConvertFrom-Json
 $peer=[NativeAcceptancePeer]::new($settings.binary,$settings.home,$settings.binarySHA256,[int]$settings.timeoutMs,[uint32]$settings.expectedOwnerPid)
 [Console]::WriteLine((@{kind='ready';canonicalHome=$peer.canonicalHome;logonSid=$peer.logonSid;elapsedMs=$peer.ElapsedMs}|ConvertTo-Json -Compress))
 $sequence=0
 while($true){
  $read=[Console]::In.ReadLineAsync()
  if(-not $read.Wait($peer.RemainingMs())){throw 'peer.input_deadline'}
  $line=$read.Result
  if($null -eq $line){throw 'peer.controller_disconnected'}
  if($line.Length -gt 32768){throw 'peer.command_budget'}
  $command=$line|ConvertFrom-Json
  $sequence+=1
  if($command.sequence -ne $sequence -or $sequence -gt 900){throw 'peer.command_sequence'}
  $value=$null
  switch($command.action){
   'exchange' {$value=$peer.Request($command.pipeName,$command.requestPath,[int]$command.timeoutMs,[bool]$command.reject)}
   'observe' {$value=$peer.Observe()}
   'wait' {$value=$peer.WaitExit([int]$command.timeoutMs)}
   'finish' {$peer.Dispose();$peer=$null;[Console]::WriteLine((@{kind='finished';sequence=$sequence;handlesClosed=$true}|ConvertTo-Json -Compress));exit 0}
   default {throw 'peer.command_action'}
  }
  $json=@{schema='lycheedev.native-session-peer-observation.v1';sequence=$sequence;action=$command.action;elapsedMs=$peer.ElapsedMs;value=$value}|ConvertTo-Json -Depth 12 -Compress
  $outputBytes=[Text.Encoding]::UTF8.GetByteCount($json)
  if($command.outputBudgetBytes -lt 1 -or $command.outputBudgetBytes -gt 8388608 -or $outputBytes -gt $command.outputBudgetBytes){throw 'peer.observation_budget'}
  $output=[IO.Path]::GetFullPath($command.output)
  $evidenceRoot=[IO.Path]::GetFullPath($settings.evidenceDir).TrimEnd('\')+'\'
  if(-not $output.StartsWith($evidenceRoot,[StringComparison]::OrdinalIgnoreCase) -or [IO.File]::Exists($output)){throw 'peer.output_scope'}
  [IO.File]::WriteAllText($output,$json,$utf8)
  [Console]::WriteLine((@{kind='result';sequence=$sequence;output=$output}|ConvertTo-Json -Compress))
 }
} catch {
 # Preserve completed reads even when authentication, transport or a later
 # assertion failed. A missing response is recorded as missing, never success.
 $failure=$_.Exception
 try {
  if($null -ne $peer -and $null -ne $command -and $command.action -eq 'exchange'){
   $failurePath=[IO.Path]::GetFullPath($command.output)
   $root=[IO.Path]::GetFullPath($settings.evidenceDir).TrimEnd('\')+'\'
   $record=@{schema='lycheedev.native-session-peer-observation.v1';sequence=$sequence;action=$command.action;elapsedMs=$peer.ElapsedMs;value=$peer.lastExchange;error=$failure.Message}|ConvertTo-Json -Depth 12 -Compress
   if($failurePath.StartsWith($root,[StringComparison]::OrdinalIgnoreCase) -and -not [IO.File]::Exists($failurePath) -and [Text.Encoding]::UTF8.GetByteCount($record) -le $command.outputBudgetBytes){[IO.File]::WriteAllText($failurePath,$record,$utf8)}
  }
 } catch {}
 [Console]::Error.WriteLine($failure.ToString())
 [Console]::WriteLine((@{kind='failure';message=$failure.Message}|ConvertTo-Json -Compress))
 exit 1
} finally {if($null -ne $peer){$peer.Dispose()}}
