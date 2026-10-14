param([Parameter(Mandatory=$true)][string]$Request)
$ErrorActionPreference='Stop'
$requestPath=(Resolve-Path -LiteralPath $Request).Path
if((Get-Item -LiteralPath $requestPath).Length -gt 1048576){throw 'owned.request_budget'}
$requestData=[IO.File]::ReadAllText($requestPath)|ConvertFrom-Json
$binary=(Get-Command -Name $requestData.command -CommandType Application -ErrorAction Stop | Select-Object -First 1).Source
$helperAdmission=@{state='disabled';reason='ordinary_process';helpers=@()}
$heldToolFiles=New-Object 'System.Collections.Generic.List[System.IO.FileStream]'
$admittedHelperPaths=@()
# Avoid inheriting an incompatible PowerShell Security module from a pwsh 7
# caller into Windows PowerShell. Verify the Windows trust provider directly.
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Security.Cryptography.X509Certificates;
public static class ToolkitSignature {
 [StructLayout(LayoutKind.Sequential,CharSet=CharSet.Unicode)] struct FILE {public uint size;[MarshalAs(UnmanagedType.LPWStr)]public string path;public IntPtr file,subject;}
 [StructLayout(LayoutKind.Sequential)] struct DATA {public uint size;public IntPtr policy,sip;public uint ui,revocation,choice;public IntPtr file;public uint action;public IntPtr state,url;public uint flags,context;public IntPtr settings;}
 [DllImport("wintrust.dll",ExactSpelling=true,SetLastError=false)] static extern int WinVerifyTrust(IntPtr window,ref Guid action,ref DATA data);
 public sealed class Identity {public string Subject,Thumbprint;}
 public static Identity Verify(string path){
  var action=new Guid("00AAC56B-CD44-11d0-8CC2-00C04FC295EE");var file=new FILE{size=(uint)Marshal.SizeOf(typeof(FILE)),path=path};IntPtr pointer=Marshal.AllocHGlobal(Marshal.SizeOf(typeof(FILE)));
  var data=new DATA{size=(uint)Marshal.SizeOf(typeof(DATA)),ui=2,choice=1,file=pointer,action=1,flags=0x1000};
  try{Marshal.StructureToPtr(file,pointer,false);int status=WinVerifyTrust(new IntPtr(-1),ref action,ref data);if(status!=0)throw new Exception("owned.authenticode:"+status.ToString("X8"));using(var certificate=new X509Certificate2(X509Certificate.CreateFromSignedFile(path))){return new Identity{Subject=certificate.Subject,Thumbprint=certificate.Thumbprint};}}
  finally{data.action=2;WinVerifyTrust(new IntPtr(-1),ref action,ref data);Marshal.DestroyStructure(pointer,typeof(FILE));Marshal.FreeHGlobal(pointer);}
 }
}
'@
function Open-IdentityFile([string]$path) {
 $cursor=[IO.Path]::GetFullPath($path)
 while($cursor){if(([IO.File]::GetAttributes($cursor) -band [IO.FileAttributes]::ReparsePoint) -ne 0){throw 'owned.tool_reparse'};$cursor=[IO.Path]::GetDirectoryName($cursor)}
 $file=[IO.File]::Open($path,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::Read)
 $heldToolFiles.Add($file);$digest=[Security.Cryptography.SHA256]::Create()
 try{$hash=[BitConverter]::ToString($digest.ComputeHash($file)).Replace('-','').ToLowerInvariant()}finally{$digest.Dispose();$file.Position=0}
 return @{path=[IO.Path]::GetFullPath($path);bytes=$file.Length;sha256=$hash}
}
# Only this reviewed C compile/link shape can request the MSVC helper path.
# In particular, response files and /B1,/B2 tool replacement are not admitted.
function Assert-MsvcCompileArguments($arguments,[string]$cwd) {
 $args=@($arguments);$prefix=@('/nologo','/Bv','/O2','/MT','/Brepro','/TC');$suffix=@('/link','/Brepro','/INCREMENTAL:NO','/OPT:REF','/OPT:ICF')
 if($args.Count -lt 13 -or $args.Count -gt 140){throw 'owned.not_msvc_c_compile'}
 for($i=0;$i -lt $prefix.Count;$i++){if($args[$i] -cne $prefix[$i]){throw 'owned.not_msvc_c_compile'}}
 for($i=0;$i -lt $suffix.Count;$i++){if($args[$args.Count-$suffix.Count+$i] -cne $suffix[$i]){throw 'owned.not_msvc_c_compile'}}
 $output=[string]$args[6]
 if(-not $output.StartsWith('/Fe:',[StringComparison]::Ordinal) -or -not [IO.Path]::IsPathRooted($output.Substring(4)) -or [IO.Path]::GetExtension($output.Substring(4)) -ine '.exe'){throw 'owned.not_msvc_c_compile'}
 $sourceCount=$args.Count-$prefix.Count-$suffix.Count-1
 if($sourceCount -lt 1 -or $sourceCount -gt 128){throw 'owned.not_msvc_c_compile'}
 $seen=New-Object 'System.Collections.Generic.HashSet[string]' ([StringComparer]::OrdinalIgnoreCase)
 for($i=7;$i -lt $args.Count-$suffix.Count;$i++){
  $name=[string]$args[$i]
  if($name -cnotmatch '^[A-Za-z_][A-Za-z0-9_]*\.c$' -or -not $seen.Add($name) -or -not [IO.File]::Exists((Join-Path $cwd $name))){throw 'owned.not_msvc_c_compile'}
 }
 foreach($name in @('CL','_CL_','LINK','_LINK_')){if(-not [string]::IsNullOrEmpty([Environment]::GetEnvironmentVariable($name))){throw 'owned.msvc_environment_override'}}
}
function Open-ReviewedMsvcTool($entry,[string]$kind) {
 if($entry.kind -cne $kind -or [string]::IsNullOrEmpty($entry.programFilesPath) -or [IO.Path]::IsPathRooted($entry.programFilesPath) -or $entry.programFilesPath -match '(^|[\\/])\.\.([\\/]|$)'){throw 'owned.invalid_msvc_policy'}
 $path=Join-Path ([Environment]::GetFolderPath('ProgramFiles')) $entry.programFilesPath
 $identity=Open-IdentityFile $path
 if($identity.bytes -ne $entry.bytes -or $identity.sha256 -cne $entry.sha256){throw 'owned.unreviewed_msvc_identity'}
 $version=[Diagnostics.FileVersionInfo]::GetVersionInfo($path).FileVersion;$signature=[ToolkitSignature]::Verify($path)
 if($version -cne $entry.fileVersion -or $signature.Subject -cne $entry.signerSubject -or $signature.Thumbprint -cne $entry.signerThumbprint){throw 'owned.msvc_signature_identity'}
 $identity.kind=$kind;$identity.fileVersion=$version;$identity.signatureStatus='WinVerifyTrust:0';$identity.signerSubject=$signature.Subject;$identity.signerThumbprint=$signature.Thumbprint
 return $identity
}
if($requestData.msvcCompile -eq $true){
 try{
  if($requestData.cargoBuild -eq $true){throw 'owned.conflicting_auxiliary_request'}
  if([IO.Path]::GetFileName($binary) -ine 'cl.exe'){throw 'owned.not_msvc_compile'}
  Assert-MsvcCompileArguments $requestData.args ([string]$requestData.cwd)
  $policyPath=Join-Path $PSScriptRoot 'rust-build-auxiliary.json';$policyRaw=[IO.File]::ReadAllBytes($policyPath)
  if($policyRaw.Length -gt 16384){throw 'owned.auxiliary_policy_budget'}
  $policy=[Text.Encoding]::UTF8.GetString($policyRaw)|ConvertFrom-Json
  if($policy.schema -ne 'lycheedev.build-auxiliary-policy.v1'){throw 'owned.auxiliary_policy_schema'}
  if(@($policy.msvcCompilers).Count -lt 1 -or @($policy.msvcCompilers).Count -gt 4){throw 'owned.no_reviewed_msvc_compiler'}
  $matches=@($policy.msvcCompilers|Where-Object {[string]::Equals([IO.Path]::GetFullPath((Join-Path ([Environment]::GetFolderPath('ProgramFiles')) $_.programFilesPath)),[IO.Path]::GetFullPath($binary),[StringComparison]::OrdinalIgnoreCase)})
  if($matches.Count -ne 1){throw 'owned.unreviewed_msvc_path'}
  $compiler=$matches[0];$clIdentity=Open-ReviewedMsvcTool $compiler 'msvc-cl'
  $linkIdentity=Open-ReviewedMsvcTool $compiler.linker 'msvc-link'
  $helperMatches=@($policy.helpers|Where-Object {$_.programFilesPath -ceq $compiler.helperProgramFilesPath})
  if($helperMatches.Count -ne 1){throw 'owned.unreviewed_msvc_helper'}
  $helperIdentity=Open-ReviewedMsvcTool $helperMatches[0] 'msvc-vctip'
  $directory=[IO.Path]::GetDirectoryName($clIdentity.path)
  if(-not [string]::Equals($directory,[IO.Path]::GetDirectoryName($linkIdentity.path),[StringComparison]::OrdinalIgnoreCase) -or -not [string]::Equals($directory,[IO.Path]::GetDirectoryName($helperIdentity.path),[StringComparison]::OrdinalIgnoreCase)){throw 'owned.msvc_toolchain_directory'}
  $helperAdmission=@{state='enabled';kind='msvc-c-compile';compiler=$clIdentity;linker=$linkIdentity;helpers=@($helperIdentity)}
  $admittedHelperPaths=@($helperIdentity.path)
 }catch{$admittedHelperPaths=@();$helperAdmission=@{state='disabled';reason=$_.Exception.Message;helpers=@()}}
}elseif($requestData.cargoBuild -eq $true){
 try{
  if([IO.Path]::GetFileName($binary) -ine 'cargo.exe' -or @('build','check','test','clippy') -notcontains $requestData.args[0]){throw 'owned.not_cargo_build'}
  $policyPath=Join-Path $PSScriptRoot 'rust-build-auxiliary.json';$policyRaw=[IO.File]::ReadAllBytes($policyPath)
  if($policyRaw.Length -gt 16384){throw 'owned.auxiliary_policy_budget'}
  $policy=[Text.Encoding]::UTF8.GetString($policyRaw)|ConvertFrom-Json
  if($policy.schema -ne 'lycheedev.build-auxiliary-policy.v1'){throw 'owned.auxiliary_policy_schema'}
  $cargoIdentity=Open-IdentityFile $binary
  if($cargoIdentity.sha256 -ne $policy.cargoSHA256){throw 'owned.unreviewed_cargo_identity'}
  $helperAdmission=@{state='enabled';cargo=$cargoIdentity;helpers=@()}
  if(@($policy.helpers).Count -lt 1 -or @($policy.helpers).Count -gt 8){throw 'owned.auxiliary_policy_budget'}
  foreach($helper in $policy.helpers){
   $helperPath=Join-Path ([Environment]::GetFolderPath('ProgramFiles')) $helper.programFilesPath
   # The reviewed Community and hosted Enterprise variants need not coexist.
   # This never admits another path: every present variant still requires all
   # pinned bytes, version, signature and held-file identity checks below.
   if(-not [IO.File]::Exists($helperPath)){continue}
   $identity=Open-IdentityFile $helperPath
   if($identity.bytes -ne $helper.bytes -or $identity.sha256 -ne $helper.sha256){throw 'owned.unreviewed_helper_identity'}
   $version=[Diagnostics.FileVersionInfo]::GetVersionInfo($helperPath).FileVersion
   $signature=[ToolkitSignature]::Verify($helperPath)
   if($version -ne $helper.fileVersion -or $signature.Subject -ne $helper.signerSubject -or $signature.Thumbprint -ne $helper.signerThumbprint){throw 'owned.helper_signature_identity'}
   $identity.kind=$helper.kind;$identity.fileVersion=$version;$identity.signatureStatus='WinVerifyTrust:0';$identity.signerSubject=$signature.Subject;$identity.signerThumbprint=$signature.Thumbprint
   $helperAdmission.helpers+=@($identity);$admittedHelperPaths+=@($identity.path)
  }
  if($admittedHelperPaths.Count -eq 0){throw 'owned.no_reviewed_helper_present'}
 }catch{$admittedHelperPaths=@();$helperAdmission=@{state='disabled';reason=$_.Exception.Message;helpers=@()}}
}

# Process creation is suspended until assignment. Descendants inherit this
# non-breakaway Job, and ACTIVE_PROCESS_ZERO is the kernel tree-exit proof.
Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading.Tasks;
using System.Threading;
using Microsoft.Win32.SafeHandles;
using System.Collections.Generic;
public static class ToolkitOwnedProcess {
 [StructLayout(LayoutKind.Sequential)] struct SA {public int size;public IntPtr descriptor;public int inherit;}
 [StructLayout(LayoutKind.Sequential,CharSet=CharSet.Unicode)] struct SI {public int size;public string reserved,desktop,title;public uint x,y,xSize,ySize,xChars,yChars,fill,flags;public short show,reserved2;public IntPtr reservedPtr,input,output,error;}
 [StructLayout(LayoutKind.Sequential)] struct SIX {public SI start;public IntPtr attributes;}
 [StructLayout(LayoutKind.Sequential)] struct PI {public IntPtr process,thread;public uint pid,tid;}
 [StructLayout(LayoutKind.Sequential)] struct BASIC {public long processTime,jobTime;public uint flags;public UIntPtr min,max;public uint active;public UIntPtr affinity;public uint priority,scheduling;}
 [StructLayout(LayoutKind.Sequential)] struct IOCOUNTERS {public ulong reads,writes,others,readBytes,writeBytes,otherBytes;}
 [StructLayout(LayoutKind.Sequential)] struct LIMITS {public BASIC basic;public IOCOUNTERS io;public UIntPtr processMemory,jobMemory,peakProcess,peakJob;}
 [StructLayout(LayoutKind.Sequential)] struct PORT {public IntPtr key,port;}
 [StructLayout(LayoutKind.Sequential)] struct ACCOUNTING {public long user,kernel,periodUser,periodKernel;public uint faults,total,active,terminated;}
 [StructLayout(LayoutKind.Sequential)] struct FT {public uint low,high;}
 public sealed class ObservedProcess {public long pid,creationFileTime;public string image,error;public bool exited,jobMember,reviewedAuxiliary;}
 public sealed class Result {public uint pid,exitCode=259,activeProcesses;public long creationFileTime;public bool rootReaped,treeReaped,handlesClosed,outputReadersComplete,outputLimit,forced,deadlineHit,parentExitedBeforeChildren,drainCompletedByAccounting,knownAuxiliaryCleanup;public string error;public List<ObservedProcess> atDrainDeadline,auxiliaryProcesses;}
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern IntPtr CreateJobObjectW(IntPtr security,string name);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool SetInformationJobObject(IntPtr job,int kind,IntPtr data,uint size);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool QueryInformationJobObject(IntPtr job,int kind,out ACCOUNTING data,uint size,IntPtr returned);
 [DllImport("kernel32.dll",SetLastError=true,EntryPoint="QueryInformationJobObject")] static extern bool QueryProcessList(IntPtr job,int kind,IntPtr data,uint size,IntPtr returned);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool AssignProcessToJobObject(IntPtr job,IntPtr process);
 [DllImport("kernel32.dll",SetLastError=true)] static extern IntPtr OpenProcess(uint access,bool inherit,uint pid);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool IsProcessInJob(IntPtr process,IntPtr job,out bool member);
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool QueryFullProcessImageNameW(IntPtr process,uint flags,StringBuilder image,ref uint size);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool TerminateJobObject(IntPtr job,uint exit);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool TerminateProcess(IntPtr process,uint exit);
 [DllImport("kernel32.dll",SetLastError=true)] static extern IntPtr CreateIoCompletionPort(IntPtr file,IntPtr port,IntPtr key,uint concurrency);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool GetQueuedCompletionStatus(IntPtr port,out uint bytes,out IntPtr key,out IntPtr overlapped,uint timeout);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool PostQueuedCompletionStatus(IntPtr port,uint bytes,IntPtr key,IntPtr overlapped);
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool CreateProcessW(string app,StringBuilder line,IntPtr ps,IntPtr ts,bool inherit,uint flags,IntPtr environment,string cwd,ref SIX start,out PI info);
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern IntPtr CreateFileW(string path,uint access,uint sharing,ref SA sa,uint disposition,uint flags,IntPtr template);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool CreatePipe(out IntPtr read,out IntPtr write,ref SA security,uint size);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool SetHandleInformation(IntPtr handle,uint mask,uint flags);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool InitializeProcThreadAttributeList(IntPtr list,int count,int flags,ref IntPtr size);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool UpdateProcThreadAttribute(IntPtr list,uint flags,IntPtr attribute,IntPtr value,IntPtr size,IntPtr previous,IntPtr returned);
 [DllImport("kernel32.dll")] static extern void DeleteProcThreadAttributeList(IntPtr list);
 [DllImport("kernel32.dll",SetLastError=true)] static extern uint ResumeThread(IntPtr thread);
 [DllImport("kernel32.dll",SetLastError=true)] static extern uint WaitForSingleObject(IntPtr handle,uint timeout);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool GetExitCodeProcess(IntPtr process,out uint code);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool GetProcessTimes(IntPtr process,out FT creation,out FT exit,out FT kernel,out FT user);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool CloseHandle(IntPtr handle);
 static Exception Error(string step){return new Win32Exception(Marshal.GetLastWin32Error(),step);}
 static void Set<T>(IntPtr job,int kind,T value){IntPtr bytes=Marshal.AllocHGlobal(Marshal.SizeOf(typeof(T)));try{Marshal.StructureToPtr(value,bytes,false);if(!SetInformationJobObject(job,kind,bytes,(uint)Marshal.SizeOf(typeof(T))))throw Error("owned.job_information");}finally{Marshal.FreeHGlobal(bytes);}}
 static uint Active(IntPtr job){ACCOUNTING data;if(!QueryInformationJobObject(job,1,out data,(uint)Marshal.SizeOf(typeof(ACCOUNTING)),IntPtr.Zero))throw Error("owned.job_accounting");return data.active;}
 static List<ObservedProcess> Observe(IntPtr job){
  var result=new List<ObservedProcess>();IntPtr data=Marshal.AllocHGlobal(65536);
  try{if(!QueryProcessList(job,3,data,65536,IntPtr.Zero))throw Error("owned.job_process_list");uint count=(uint)Marshal.ReadInt32(data,4);if(count>4096)throw new Exception("owned.process_list_budget");
   for(int i=0;i<count;i++){long pid=Marshal.ReadIntPtr(data,8+i*IntPtr.Size).ToInt64();var row=new ObservedProcess{pid=pid};IntPtr handle=IntPtr.Zero;
    try{handle=OpenProcess(0x00101000,false,(uint)pid);if(handle==IntPtr.Zero)throw Error("owned.observe_open");row.exited=WaitForSingleObject(handle,0)==0;bool member;FT creation,exit,kernel,user;var image=new StringBuilder(32768);uint size=32768;
     if(!IsProcessInJob(handle,job,out member)||!GetProcessTimes(handle,out creation,out exit,out kernel,out user))throw Error("owned.observe_identity");row.jobMember=member;row.creationFileTime=((long)creation.high<<32)|creation.low;
     if(QueryFullProcessImageNameW(handle,0,image,ref size))row.image=image.ToString();else if(!row.exited)throw Error("owned.observe_image");
    }catch(Exception failure){row.error=failure.Message;}finally{if(handle!=IntPtr.Zero&&!CloseHandle(handle))throw Error("owned.close_observation_handle");}result.Add(row);
   }
  }finally{Marshal.FreeHGlobal(data);}return result;
 }
 static bool RetireReviewedAuxiliary(IntPtr job,IntPtr root,long rootCreated,string[] reviewed,Result result){
  uint rootExit;if(reviewed.Length==0||WaitForSingleObject(root,0)!=0||!GetExitCodeProcess(root,out rootExit)||rootExit!=0)return false;
  var held=new List<IntPtr>();var rows=new List<ObservedProcess>();var livePids=new HashSet<long>();
  try{
   var observed=Observe(job);if(observed.Count==0||observed.Count>16)return false;
   foreach(var item in observed){
    IntPtr handle=OpenProcess(0x00101001,false,(uint)item.pid);if(handle==IntPtr.Zero)return false;held.Add(handle);
    if(WaitForSingleObject(handle,0)==0)continue;
    bool member;FT creation,exit,kernel,user;var image=new StringBuilder(32768);uint size=32768;
    if(!IsProcessInJob(handle,job,out member)||!member||!GetProcessTimes(handle,out creation,out exit,out kernel,out user)||!QueryFullProcessImageNameW(handle,0,image,ref size))return false;
    var row=new ObservedProcess{pid=item.pid,creationFileTime=((long)creation.high<<32)|creation.low,image=image.ToString(),jobMember=member};
    row.reviewedAuxiliary=row.creationFileTime>=rootCreated&&Array.Exists(reviewed,path=>String.Equals(Path.GetFullPath(path),Path.GetFullPath(row.image),StringComparison.OrdinalIgnoreCase));
    if(!row.reviewedAuxiliary)return false;rows.Add(row);livePids.Add(row.pid);
   }
   if(rows.Count==0)return false;
   foreach(var item in Observe(job))if(!item.exited&&!livePids.Contains(item.pid))return false;
   result.auxiliaryProcesses=rows;result.forced=true;
   foreach(IntPtr handle in held)if(WaitForSingleObject(handle,0)!=0&&!TerminateProcess(handle,0))throw Error("owned.terminate_reviewed_auxiliary");
   var deadline=Stopwatch.StartNew();foreach(IntPtr handle in held)if(WaitForSingleObject(handle,(uint)Math.Max(0,10000-deadline.ElapsedMilliseconds))!=0)throw new Exception("owned.auxiliary_reap_unverified");
   return true;
  }finally{foreach(IntPtr handle in held)if(!CloseHandle(handle))throw Error("owned.close_auxiliary_handle");}
 }
 static string Quote(string argument){if(argument.Length>0&&argument.IndexOfAny(new char[]{' ','\t','\n','\v','"'})<0)return argument;var text=new StringBuilder("\"");int slashes=0;foreach(char c in argument){if(c=='\\'){slashes++;continue;}if(c=='"'){text.Append('\\',slashes*2+1);text.Append(c);}else{text.Append('\\',slashes);text.Append(c);}slashes=0;}text.Append('\\',slashes*2);text.Append('"');return text.ToString();}
 public static Result Run(string binary,string[] args,string cwd,string stdout,string stderr,int timeoutMs,int orphanMs,string[] reviewedAuxiliary){
  var result=new Result();IntPtr job=IntPtr.Zero,port=IntPtr.Zero,output=IntPtr.Zero,error=IntPtr.Zero,outputRead=IntPtr.Zero,errorRead=IntPtr.Zero,input=IntPtr.Zero,attributes=IntPtr.Zero,handles=IntPtr.Zero;PI process=new PI();bool assigned=false,attributeReady=false;var clock=Stopwatch.StartNew();long orphanDeadline=Int64.MaxValue;Task rootWatcher=null,stdoutReader=null,stderrReader=null;long outputBytes=0;string readerError=null;
  try{
   job=CreateJobObjectW(IntPtr.Zero,null);if(job==IntPtr.Zero)throw Error("owned.create_job");
   var limits=new LIMITS();limits.basic.flags=0x2000;Set(job,9,limits);
   port=CreateIoCompletionPort(new IntPtr(-1),IntPtr.Zero,IntPtr.Zero,1);if(port==IntPtr.Zero)throw Error("owned.create_port");Set(job,7,new PORT{key=job,port=port});
   var security=new SA{size=Marshal.SizeOf(typeof(SA)),inherit=1};
   if(!CreatePipe(out outputRead,out output,ref security,65536)||!CreatePipe(out errorRead,out error,ref security,65536))throw Error("owned.output_pipes");
   if(!SetHandleInformation(outputRead,1,0)||!SetHandleInformation(errorRead,1,0))throw Error("owned.pipe_inheritance");
   input=CreateFileW("NUL",0x80000000,3,ref security,3,0x80,IntPtr.Zero);if(input==new IntPtr(-1))throw Error("owned.input");
   IntPtr size=IntPtr.Zero;InitializeProcThreadAttributeList(IntPtr.Zero,1,0,ref size);attributes=Marshal.AllocHGlobal(size);if(!InitializeProcThreadAttributeList(attributes,1,0,ref size))throw Error("owned.attribute_list");attributeReady=true;
   handles=Marshal.AllocHGlobal(3*IntPtr.Size);Marshal.WriteIntPtr(handles,0,input);Marshal.WriteIntPtr(handles,IntPtr.Size,output);Marshal.WriteIntPtr(handles,2*IntPtr.Size,error);
   if(!UpdateProcThreadAttribute(attributes,0,new IntPtr(0x20002),handles,new IntPtr(3*IntPtr.Size),IntPtr.Zero,IntPtr.Zero))throw Error("owned.handle_list");
   var start=new SIX{start=new SI{size=Marshal.SizeOf(typeof(SIX)),flags=0x101,show=0,input=input,output=output,error=error},attributes=attributes};var command=new StringBuilder(Quote(binary));foreach(string arg in args)command.Append(" ").Append(Quote(arg));
   if(!CreateProcessW(binary,command,IntPtr.Zero,IntPtr.Zero,true,0x08080004,IntPtr.Zero,cwd,ref start,out process))throw Error("owned.create_suspended");
   result.pid=process.pid;FT created,exited,kernel,user;if(!GetProcessTimes(process.process,out created,out exited,out kernel,out user))throw Error("owned.process_identity");result.creationFileTime=((long)created.high<<32)|created.low;
   if(!AssignProcessToJobObject(job,process.process))throw Error("owned.assign_job");assigned=true;
   Action<IntPtr,string> pump=(handle,path)=>{try{using(var stream=new FileStream(new SafeFileHandle(handle,false),FileAccess.Read,65536,false))using(var file=new FileStream(path,FileMode.CreateNew,FileAccess.Write,FileShare.Read)){var buffer=new byte[65536];int count;while((count=stream.Read(buffer,0,buffer.Length))>0){if(Interlocked.Add(ref outputBytes,count)>16*1024*1024){result.outputLimit=true;TerminateJobObject(job,8);break;}file.Write(buffer,0,count);}}}catch(Exception failure){Interlocked.CompareExchange(ref readerError,failure.ToString(),null);TerminateJobObject(job,8);}};
   stdoutReader=Task.Run(()=>pump(outputRead,stdout));stderrReader=Task.Run(()=>pump(errorRead,stderr));
   if(!CloseHandle(output)||!CloseHandle(error))throw Error("owned.close_parent_writers");output=IntPtr.Zero;error=IntPtr.Zero;
   if(ResumeThread(process.thread)==UInt32.MaxValue)throw Error("owned.resume");
   rootWatcher=Task.Run(()=>{if(WaitForSingleObject(process.process,0xffffffff)==0)PostQueuedCompletionStatus(port,0x80000001,job,new IntPtr(process.pid));});
   while(true){
    long remaining=Math.Min(timeoutMs-clock.ElapsedMilliseconds,orphanDeadline-clock.ElapsedMilliseconds);
    if(remaining<=0){if(Active(job)==0){result.drainCompletedByAccounting=true;break;}result.atDrainDeadline=Observe(job);if(Active(job)==0){result.drainCompletedByAccounting=true;break;}result.forced=true;result.deadlineHit=clock.ElapsedMilliseconds>=timeoutMs;if(!TerminateJobObject(job,8))throw Error("owned.terminate_job");break;}
    uint message;IntPtr key,value;bool received=GetQueuedCompletionStatus(port,out message,out key,out value,(uint)Math.Min(remaining,Int32.MaxValue));
    if(!received){int code=Marshal.GetLastWin32Error();if(code==258)continue;throw Error("owned.wait_job");}
    if(key!=job)throw new Exception("owned.foreign_completion");
    if(message==4)break;
    if((message==7||message==8||message==0x80000001)&&(uint)value.ToInt64()==process.pid){result.rootReaped=WaitForSingleObject(process.process,0)==0;if(result.rootReaped&&Active(job)>0){result.parentExitedBeforeChildren=true;if(clock.ElapsedMilliseconds<timeoutMs&&!result.outputLimit&&RetireReviewedAuxiliary(job,process.process,result.creationFileTime,reviewedAuxiliary,result)){result.knownAuxiliaryCleanup=true;break;}orphanDeadline=Math.Min(orphanDeadline,clock.ElapsedMilliseconds+orphanMs);}}
   }
   if(result.forced){var deadline=clock.ElapsedMilliseconds+10000;while(Active(job)>0&&clock.ElapsedMilliseconds<deadline){uint message;IntPtr key,value;GetQueuedCompletionStatus(port,out message,out key,out value,(uint)Math.Max(1,deadline-clock.ElapsedMilliseconds));}}
   result.activeProcesses=Active(job);result.treeReaped=result.activeProcesses==0;result.rootReaped=WaitForSingleObject(process.process,10000)==0;
   result.outputReadersComplete=Task.WaitAll(new Task[]{stdoutReader,stderrReader},10000);
   if(readerError!=null)throw new Exception(readerError);if(result.outputLimit){result.forced=true;result.error="owned.output_budget";}
   if(!GetExitCodeProcess(process.process,out result.exitCode))throw Error("owned.exit_code");
  }catch(Exception failure){result.error=failure.ToString();}
  finally{
   if(process.process!=IntPtr.Zero&&!result.treeReaped){
    result.knownAuxiliaryCleanup=false;
    try{result.forced=true;if(assigned){TerminateJobObject(job,8);var deadline=clock.ElapsedMilliseconds+10000;while(Active(job)>0&&clock.ElapsedMilliseconds<deadline){uint message;IntPtr key,value;GetQueuedCompletionStatus(port,out message,out key,out value,(uint)Math.Max(1,deadline-clock.ElapsedMilliseconds));}result.activeProcesses=Active(job);result.treeReaped=result.activeProcesses==0;}else{TerminateProcess(process.process,8);result.treeReaped=WaitForSingleObject(process.process,10000)==0;}result.rootReaped=WaitForSingleObject(process.process,10000)==0;GetExitCodeProcess(process.process,out result.exitCode);}catch(Exception cleanup){result.error=(result.error??"")+"\n"+cleanup.ToString();}
   }
   if(output!=IntPtr.Zero&&output!=new IntPtr(-1)){CloseHandle(output);output=IntPtr.Zero;}if(error!=IntPtr.Zero&&error!=new IntPtr(-1)){CloseHandle(error);error=IntPtr.Zero;}
   if(stdoutReader!=null&&stderrReader!=null)result.outputReadersComplete=Task.WaitAll(new Task[]{stdoutReader,stderrReader},10000);
   bool closed=rootWatcher==null||rootWatcher.Wait(10000);foreach(IntPtr handle in new IntPtr[]{process.thread,process.process,input,outputRead,errorRead,job,port})if(handle!=IntPtr.Zero&&handle!=new IntPtr(-1))closed=CloseHandle(handle)&&closed;
   if(attributeReady)DeleteProcThreadAttributeList(attributes);if(attributes!=IntPtr.Zero)Marshal.FreeHGlobal(attributes);if(handles!=IntPtr.Zero)Marshal.FreeHGlobal(handles);result.handlesClosed=closed;
  }
  return result;
 }
}
'@
try{$result=[ToolkitOwnedProcess]::Run($binary,[string[]]$requestData.args,[string]$requestData.cwd,[string]$requestData.stdout,[string]$requestData.stderr,[int]$requestData.timeoutMs,[int]$requestData.orphanMs,[string[]]$admittedHelperPaths)}finally{foreach($file in $heldToolFiles){$file.Dispose()}}
# FILETIME exceeds the JSON/JavaScript exact integer range. Preserve the native
# Int64 comparisons above, and emit decimal strings at every evidence boundary.
function Convert-ObservedProcesses($rows) {
 if($null -eq $rows){return $null}
 return ,@($rows | ForEach-Object {
  @{pid=$_.pid;creationFileTime=$_.creationFileTime.ToString([Globalization.CultureInfo]::InvariantCulture);image=$_.image;error=$_.error;exited=$_.exited;jobMember=$_.jobMember;reviewedAuxiliary=$_.reviewedAuxiliary}
 })
}
$receipt=@{schema='lycheedev.owned-process.v1';binary=$binary;pid=$result.pid;creationFileTime=$result.creationFileTime.ToString([Globalization.CultureInfo]::InvariantCulture);exitCode=$result.exitCode;rootReaped=$result.rootReaped;treeReaped=$result.treeReaped;handlesClosed=$result.handlesClosed;toolIdentityLocksClosed=$true;outputReadersComplete=$result.outputReadersComplete;outputLimit=$result.outputLimit;activeProcesses=$result.activeProcesses;forced=$result.forced;knownAuxiliaryCleanup=$result.knownAuxiliaryCleanup;auxiliaryProcesses=(Convert-ObservedProcesses $result.auxiliaryProcesses);helperAdmission=$helperAdmission;deadlineHit=$result.deadlineHit;parentExitedBeforeChildren=$result.parentExitedBeforeChildren;drainCompletedByAccounting=$result.drainCompletedByAccounting;atDrainDeadline=(Convert-ObservedProcesses $result.atDrainDeadline);error=$result.error}
[IO.File]::WriteAllText([string]$requestData.receipt,($receipt|ConvertTo-Json -Depth 5))
if(!$result.treeReaped -or !$result.rootReaped -or !$result.handlesClosed -or !$result.outputReadersComplete){exit 8}
exit 0
