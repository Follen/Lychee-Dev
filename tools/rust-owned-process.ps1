param([Parameter(Mandatory=$true)][string]$Request)
$ErrorActionPreference='Stop'
$requestPath=(Resolve-Path -LiteralPath $Request).Path
# Parse and hash the same held bytes, preserving StreamReader BOM compatibility.
$requestFile=[IO.File]::Open($requestPath,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::Read)
$requestReader=$null;$requestDigest=$null
try{
 if($requestFile.Length -gt 1048576){throw 'owned.request_budget'}
 $requestReader=New-Object IO.StreamReader ($requestFile)
 $requestData=$requestReader.ReadToEnd()|ConvertFrom-Json
 $requestFile.Position=0;$requestDigest=[Security.Cryptography.SHA256]::Create()
 $requestSHA256=[BitConverter]::ToString($requestDigest.ComputeHash($requestFile)).Replace('-','').ToLowerInvariant()
}finally{if($requestDigest){$requestDigest.Dispose()};if($requestReader){$requestReader.Dispose()}else{$requestFile.Dispose()}}
# Existing direct PS1 consumers use unversioned requests with caller-chosen names.
# Keep UUID basenames discoverable; otherwise generate a new identity rather than
# pretending the old request declared one. The original request bytes never change.
$name=[IO.Path]::GetFileName($requestPath)
if($null -ne $requestData.schema -and $requestData.schema -cne 'lycheedev.owned-request.v2'){throw 'owned.request_schema'}
if($name -cmatch '^process-(?<id>[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})\.request\.json$'){
 $runId=$Matches['id'];$identitySource='legacy-request-basename'
 if($requestData.schema -ceq 'lycheedev.owned-request.v2'){$identitySource='request-v2'}
}elseif($requestData.schema -ceq 'lycheedev.owned-request.v2'){throw 'owned.request_identity_required'}
else{$runId=[Guid]::NewGuid().ToString('D');$identitySource='legacy-generated'}
$expectedStem=Join-Path ([IO.Path]::GetDirectoryName($requestPath)) ('process-'+$runId)
if($null -ne $requestData.runId -and $requestData.runId -cne $runId){throw 'owned.start_identity_run'}
foreach($entry in @(@('identity',($expectedStem+'.identity.json')),@('rootIdentity',($expectedStem+'.root.json')))){
 $value=$requestData.($entry[0])
 if($null -eq $value){if($requestData.schema -ceq 'lycheedev.owned-request.v2'){throw 'owned.start_identity_path'}}
 elseif(-not [string]::Equals([IO.Path]::GetFullPath([string]$value),[IO.Path]::GetFullPath($entry[1]),[StringComparison]::OrdinalIgnoreCase)){throw 'owned.start_identity_path'}
}
if($requestData.schema -ceq 'lycheedev.owned-request.v2' -and $null -eq $requestData.runId){throw 'owned.start_identity_run'}
$requestData|Add-Member -NotePropertyMembers @{runId=$runId;identity=$expectedStem+'.identity.json';rootIdentity=$expectedStem+'.root.json'} -Force
# Startup identity is evidence, never a substitute for the terminal reap receipt.
$wrapperProcess=[Diagnostics.Process]::GetCurrentProcess()
Add-Type -TypeDefinition 'using System;using System.Runtime.InteropServices;public static class ToolkitBootCounter{[DllImport("kernel32.dll",SetLastError=true)]public static extern bool QueryUnbiasedInterruptTime(out ulong time);}'
[UInt64]$bootCounter=0
if(-not [ToolkitBootCounter]::QueryUnbiasedInterruptTime([ref]$bootCounter)){throw 'owned.boot_counter'}
$identity=@{schema='lycheedev.owned-start.v1';runId=$requestData.runId;identitySource=$identitySource;requestPath=$requestPath;bootClock='QueryUnbiasedInterruptTime';coordinatorPid=$requestData.coordinatorPid;wrapperPid=$PID;wrapperCreationFileTime=$wrapperProcess.StartTime.ToUniversalTime().ToFileTimeUtc().ToString([Globalization.CultureInfo]::InvariantCulture);bootUptime100ns=$bootCounter.ToString([Globalization.CultureInfo]::InvariantCulture);requestSHA256=$requestSHA256;wrapperObservedSHA256=(Get-FileHash -LiteralPath $PSCommandPath -Algorithm SHA256).Hash.ToLowerInvariant();rootIdentity=[string]$requestData.rootIdentity;state='wrapper-started';rootAssignment='PROC_THREAD_ATTRIBUTE_JOB_LIST';historicalReapProof=$false}
$identityBytes=[Text.Encoding]::UTF8.GetBytes(($identity|ConvertTo-Json -Depth 4))
$identityFile=New-Object IO.FileStream ([string]$requestData.identity),([IO.FileMode]::CreateNew),([IO.FileAccess]::Write),([IO.FileShare]::Read),4096,([IO.FileOptions]::WriteThrough)
try{$identityFile.Write($identityBytes,0,$identityBytes.Length);$identityFile.Flush($true)}finally{$identityFile.Dispose()}
$wrapperProcess.Dispose()
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
 public sealed class Result {public uint pid,exitCode=259,activeProcesses;public long creationFileTime,commandStartTicks,rootExitTicks,treeReapedTicks;public bool rootReaped,treeReaped,handlesClosed,outputReadersComplete,outputLimit,forced,deadlineHit,parentExitedBeforeChildren,drainCompletedByAccounting,knownAuxiliaryCleanup;public string error;public ToolkitMemory.Status memory;public List<ObservedProcess> atDrainDeadline,auxiliaryProcesses;}
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern IntPtr CreateJobObjectW(IntPtr security,string name);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool SetInformationJobObject(IntPtr job,int kind,IntPtr data,uint size);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool QueryInformationJobObject(IntPtr job,int kind,out ACCOUNTING data,uint size,IntPtr returned);
 [DllImport("kernel32.dll",SetLastError=true,EntryPoint="QueryInformationJobObject")] static extern bool QueryProcessList(IntPtr job,int kind,IntPtr data,uint size,IntPtr returned);
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
 public static Result Run(string binary,string[] args,string cwd,string stdout,string stderr,int timeoutMs,int orphanMs,string[] reviewedAuxiliary,string memoryPrefix,string memoryMarker,string rootIdentity,string runId){
  var result=new Result();IntPtr job=IntPtr.Zero,port=IntPtr.Zero,output=IntPtr.Zero,error=IntPtr.Zero,outputRead=IntPtr.Zero,errorRead=IntPtr.Zero,input=IntPtr.Zero,attributes=IntPtr.Zero,handles=IntPtr.Zero,jobHandles=IntPtr.Zero;PI process=new PI();bool assigned=false,attributeReady=false;var clock=Stopwatch.StartNew();long orphanDeadline=Int64.MaxValue;Task rootWatcher=null,stdoutReader=null,stderrReader=null;long outputBytes=0;string readerError=null;ToolkitMemory.Observer memory=null;
  try{
   job=CreateJobObjectW(IntPtr.Zero,null);if(job==IntPtr.Zero)throw Error("owned.create_job");
   var limits=new LIMITS();limits.basic.flags=0x2000;Set(job,9,limits);
   port=CreateIoCompletionPort(new IntPtr(-1),IntPtr.Zero,IntPtr.Zero,1);if(port==IntPtr.Zero)throw Error("owned.create_port");Set(job,7,new PORT{key=job,port=port});
   var security=new SA{size=Marshal.SizeOf(typeof(SA)),inherit=1};
   if(!CreatePipe(out outputRead,out output,ref security,65536)||!CreatePipe(out errorRead,out error,ref security,65536))throw Error("owned.output_pipes");
   if(!SetHandleInformation(outputRead,1,0)||!SetHandleInformation(errorRead,1,0))throw Error("owned.pipe_inheritance");
   input=CreateFileW("NUL",0x80000000,3,ref security,3,0x80,IntPtr.Zero);if(input==new IntPtr(-1))throw Error("owned.input");
   IntPtr size=IntPtr.Zero;InitializeProcThreadAttributeList(IntPtr.Zero,2,0,ref size);attributes=Marshal.AllocHGlobal(size);if(!InitializeProcThreadAttributeList(attributes,2,0,ref size))throw Error("owned.attribute_list");attributeReady=true;
   handles=Marshal.AllocHGlobal(3*IntPtr.Size);Marshal.WriteIntPtr(handles,0,input);Marshal.WriteIntPtr(handles,IntPtr.Size,output);Marshal.WriteIntPtr(handles,2*IntPtr.Size,error);
   if(!UpdateProcThreadAttribute(attributes,0,new IntPtr(0x20002),handles,new IntPtr(3*IntPtr.Size),IntPtr.Zero,IntPtr.Zero))throw Error("owned.handle_list");
   // Windows 10 / Server 2016+: assign at creation, with no unowned suspended window.
   // Keep the existing explicit inherited HANDLE_LIST. The Job handle is not inherited.
   jobHandles=Marshal.AllocHGlobal(IntPtr.Size);Marshal.WriteIntPtr(jobHandles,job);
   if(!UpdateProcThreadAttribute(attributes,0,new IntPtr(0x2000D),jobHandles,new IntPtr(IntPtr.Size),IntPtr.Zero,IntPtr.Zero))throw Error("owned.atomic_job_attribute");
   var start=new SIX{start=new SI{size=Marshal.SizeOf(typeof(SIX)),flags=0x101,show=0,input=input,output=output,error=error},attributes=attributes};var command=new StringBuilder(Quote(binary));foreach(string arg in args)command.Append(" ").Append(Quote(arg));
   // Same monotonic clock: exclude PowerShell/compiler startup, retain all CLI work.
   result.commandStartTicks=Stopwatch.GetTimestamp();
   if(!CreateProcessW(binary,command,IntPtr.Zero,IntPtr.Zero,true,0x08080004,IntPtr.Zero,cwd,ref start,out process))throw Error("owned.create_suspended");
   assigned=true; // A successful JOB_LIST CreateProcess already owns the suspended root.
   result.pid=process.pid;FT created,exited,kernel,user;if(!GetProcessTimes(process.process,out created,out exited,out kernel,out user))throw Error("owned.process_identity");result.creationFileTime=((long)created.high<<32)|created.low;
   bool member;if(!IsProcessInJob(process.process,job,out member)||!member)throw Error("owned.atomic_job_membership");
   // Persist the exact native root before allowing it to execute any user code.
   string rootJson="{\"schema\":\"lycheedev.owned-root.v1\",\"runId\":\""+runId+"\",\"pid\":"+process.pid.ToString(System.Globalization.CultureInfo.InvariantCulture)+",\"creationFileTime\":\""+result.creationFileTime.ToString(System.Globalization.CultureInfo.InvariantCulture)+"\",\"jobAssignedAtCreation\":true,\"state\":\"suspended\",\"historicalReapProof\":false}";
   using(var file=new FileStream(rootIdentity,FileMode.CreateNew,FileAccess.Write,FileShare.Read,4096,FileOptions.WriteThrough)){byte[] bytes=new UTF8Encoding(false).GetBytes(rootJson);file.Write(bytes,0,bytes.Length);file.Flush(true);}
   Action<IntPtr,string> pump=(handle,path)=>{try{using(var stream=new FileStream(new SafeFileHandle(handle,false),FileAccess.Read,65536,false))using(var file=new FileStream(path,FileMode.CreateNew,FileAccess.Write,FileShare.Read)){var buffer=new byte[65536];int count;while((count=stream.Read(buffer,0,buffer.Length))>0){if(Interlocked.Add(ref outputBytes,count)>16*1024*1024){result.outputLimit=true;TerminateJobObject(job,8);break;}file.Write(buffer,0,count);}}}catch(Exception failure){Interlocked.CompareExchange(ref readerError,failure.ToString(),null);TerminateJobObject(job,8);}};
   stdoutReader=Task.Run(()=>pump(outputRead,stdout));stderrReader=Task.Run(()=>pump(errorRead,stderr));
   if(!CloseHandle(output)||!CloseHandle(error))throw Error("owned.close_parent_writers");output=IntPtr.Zero;error=IntPtr.Zero;
   // Observation failure is evidence-only and must not abort the business run.
   if(!String.IsNullOrEmpty(memoryPrefix))try{memory=new ToolkitMemory.Observer(job,process.pid,result.creationFileTime,memoryPrefix,memoryMarker);}catch(Exception failure){result.memory=new ToolkitMemory.Status{error=failure.ToString(),threadStopped=true,handlesClosed=!failure.Message.Contains("memory.close_startup_job")};}
   if(ResumeThread(process.thread)==UInt32.MaxValue)throw Error("owned.resume");
   rootWatcher=Task.Run(()=>{if(WaitForSingleObject(process.process,0xffffffff)==0){Interlocked.CompareExchange(ref result.rootExitTicks,Stopwatch.GetTimestamp(),0);PostQueuedCompletionStatus(port,0x80000001,job,new IntPtr(process.pid));}});
   while(true){
    long remaining=Math.Min(timeoutMs-clock.ElapsedMilliseconds,orphanDeadline-clock.ElapsedMilliseconds);
    if(remaining<=0){
     if(Active(job)==0){result.drainCompletedByAccounting=true;break;}
     result.atDrainDeadline=Observe(job);
     if(Active(job)==0){result.drainCompletedByAccounting=true;break;}
     // Ordinary descendants may have exited since the root notification.
     // Reuse the complete authenticated-handle review only within the original
     // hard budget; unknown children and prior failures still fail closed.
     if(result.rootReaped&&clock.ElapsedMilliseconds<timeoutMs&&!result.outputLimit&&readerError==null&&result.error==null&&RetireReviewedAuxiliary(job,process.process,result.creationFileTime,reviewedAuxiliary,result)){result.knownAuxiliaryCleanup=true;break;}
     if(Active(job)==0){result.drainCompletedByAccounting=true;break;}
     result.forced=true;result.deadlineHit=clock.ElapsedMilliseconds>=timeoutMs;
     if(!TerminateJobObject(job,8))throw Error("owned.terminate_job");break;
    }
    uint message;IntPtr key,value;bool received=GetQueuedCompletionStatus(port,out message,out key,out value,(uint)Math.Min(remaining,Int32.MaxValue));
    if(!received){int code=Marshal.GetLastWin32Error();if(code==258)continue;throw Error("owned.wait_job");}
    if(key!=job)throw new Exception("owned.foreign_completion");
    if(message==4)break;
    if((message==7||message==8||message==0x80000001)&&(uint)value.ToInt64()==process.pid){result.rootReaped=WaitForSingleObject(process.process,0)==0;if(result.rootReaped&&Active(job)>0){result.parentExitedBeforeChildren=true;if(clock.ElapsedMilliseconds<timeoutMs&&!result.outputLimit&&readerError==null&&result.error==null&&RetireReviewedAuxiliary(job,process.process,result.creationFileTime,reviewedAuxiliary,result)){result.knownAuxiliaryCleanup=true;break;}orphanDeadline=Math.Min(orphanDeadline,clock.ElapsedMilliseconds+orphanMs);}}
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
   // Do not include observer shutdown in the product interval. A successful
   // receipt still requires observer, watcher and native handle closure below.
   if(result.rootReaped&&result.treeReaped&&result.outputReadersComplete){
    Interlocked.CompareExchange(ref result.rootExitTicks,Stopwatch.GetTimestamp(),0);
    result.treeReapedTicks=Stopwatch.GetTimestamp();
   }
   if(memory!=null)try{result.memory=memory.Stop();}catch(Exception failure){result.memory=new ToolkitMemory.Status{error=failure.ToString()};}
   // Always wait for the original watcher, even when observation shutdown failed.
   bool watcherClosed=rootWatcher==null||rootWatcher.Wait(10000);
   bool closed=(result.memory==null||(result.memory.threadStopped&&result.memory.handlesClosed))&&watcherClosed;foreach(IntPtr handle in new IntPtr[]{process.thread,process.process,input,outputRead,errorRead,job,port})if(handle!=IntPtr.Zero&&handle!=new IntPtr(-1))closed=CloseHandle(handle)&&closed;
   if(attributeReady)DeleteProcThreadAttributeList(attributes);if(attributes!=IntPtr.Zero)Marshal.FreeHGlobal(attributes);if(handles!=IntPtr.Zero)Marshal.FreeHGlobal(handles);if(jobHandles!=IntPtr.Zero)Marshal.FreeHGlobal(jobHandles);result.handlesClosed=closed;
  }
  return result;
 }
}
// Optional observation only. Never terminates, suspends, or changes a process.
// Compiled together with rust-owned-process.ps1's owner definition.
namespace ToolkitMemory {
 using System;
 using System.Collections.Generic;
 using System.ComponentModel;
 using System.Diagnostics;
 using System.Globalization;
 using System.IO;
 using System.Runtime.InteropServices;
 using System.Text;
 using System.Threading;
 public sealed class Status {
  public string schema="lycheedev.owned-memory.v1",error;
  public int intervalMs=100,samples,completeSamples;
  public long frequency=Stopwatch.Frequency,bytes;
  public uint totalAssignedProcesses;
  public bool threadStopped,handlesClosed,budgetExceeded;
  public List<string> chunks=new List<string>();
 }
 public sealed class Observer {
  // The duplicate is query-only, non-inheritable, and owned by the sampling
  // thread. The owner may close its original Job handle without a use-after-close.
  const uint QueryJob=4,QueryProcess=0x00101000;
  const int MaxProcesses=128,MaxSamples=15000,ChunkBytes=8<<20,MaxBytes=64<<20;
  [StructLayout(LayoutKind.Sequential)] struct FT {public uint low,high;}
  [StructLayout(LayoutKind.Sequential)] struct Counters {public uint size,faults;public UIntPtr peakWorkingSet,workingSet,peakPaged,paged,peakNonPaged,nonPaged,pagefile,peakPagefile,privateUsage;}
  [StructLayout(LayoutKind.Sequential)] struct Accounting {public long user,kernel,periodUser,periodKernel;public uint faults,total,active,terminated;}
  [DllImport("kernel32.dll")] static extern IntPtr GetCurrentProcess();
  [DllImport("kernel32.dll",SetLastError=true)] static extern bool DuplicateHandle(IntPtr source,IntPtr handle,IntPtr target,out IntPtr duplicate,uint access,bool inherit,uint options);
  [DllImport("kernel32.dll",SetLastError=true)] static extern bool QueryInformationJobObject(IntPtr job,int kind,IntPtr data,uint size,IntPtr returned);
  [DllImport("kernel32.dll",SetLastError=true,EntryPoint="QueryInformationJobObject")] static extern bool QueryAccounting(IntPtr job,int kind,out Accounting data,uint size,IntPtr returned);
  [DllImport("kernel32.dll",SetLastError=true)] static extern IntPtr OpenProcess(uint access,bool inherit,uint pid);
  [DllImport("kernel32.dll",SetLastError=true)] static extern bool IsProcessInJob(IntPtr process,IntPtr job,out bool member);
  [DllImport("kernel32.dll",SetLastError=true)] static extern bool GetProcessTimes(IntPtr process,out FT creation,out FT exit,out FT kernel,out FT user);
  [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool QueryFullProcessImageNameW(IntPtr process,uint flags,StringBuilder image,ref uint size);
  [DllImport("kernel32.dll",SetLastError=true)] static extern bool K32GetProcessMemoryInfo(IntPtr process,ref Counters counters,uint size);
  [DllImport("kernel32.dll",SetLastError=true)] static extern uint WaitForSingleObject(IntPtr handle,uint timeout);
  [DllImport("kernel32.dll",SetLastError=true)] static extern bool CloseHandle(IntPtr handle);
  readonly IntPtr job; readonly uint rootPid; readonly long rootCreated;
  readonly string prefix,marker; readonly ManualResetEvent stop=new ManualResetEvent(false);
  readonly Thread thread; readonly Status status=new Status();
  FileStream file; long chunkBytes; bool closeFailed;
  static Exception Error(string step){return new Win32Exception(Marshal.GetLastWin32Error(),"memory."+step);}
  static string N(long value){return value.ToString(CultureInfo.InvariantCulture);}
  static string U(ulong value){return value.ToString(CultureInfo.InvariantCulture);}
  static string Q(string value){if(value==null)return "null";var text=new StringBuilder("\"");foreach(char c in value){if(c=='"'||c=='\\')text.Append('\\').Append(c);else if(c<32)text.Append("\\u").Append(((int)c).ToString("x4"));else text.Append(c);}return text.Append('"').ToString();}
  public Observer(IntPtr ownedJob,uint pid,long created,string outputPrefix,string phaseMarker){
   prefix=outputPrefix;marker=phaseMarker;rootPid=pid;rootCreated=created;
   IntPtr self=GetCurrentProcess();
   if(!DuplicateHandle(self,ownedJob,self,out job,QueryJob,false,0)){stop.Dispose();throw Error("duplicate_query_job");}
   thread=new Thread(Run);thread.IsBackground=true;
   try{thread.Start();}catch{bool closed=CloseHandle(job);stop.Dispose();if(!closed)throw new Exception("memory.close_startup_job");throw;}
  }
  string Phase(){
   if(!File.Exists(marker))return null;
   using(var input=new FileStream(marker,FileMode.Open,FileAccess.Read,FileShare.ReadWrite|FileShare.Delete)){
    if(input.Length>1024)throw new Exception("memory.phase_budget");
    using(var reader=new StreamReader(input,new UTF8Encoding(false,true)))return reader.ReadToEnd();
   }
  }
  List<long> Members(){
   int size=8+MaxProcesses*IntPtr.Size;IntPtr data=Marshal.AllocHGlobal(size);
   try{
    if(!QueryInformationJobObject(job,3,data,(uint)size,IntPtr.Zero))throw Error("job_process_list");
    int assigned=Marshal.ReadInt32(data,0),count=Marshal.ReadInt32(data,4);
    if(count<0||count>MaxProcesses||assigned!=count)throw new Exception("memory.process_list_budget_or_race");
    var result=new List<long>();for(int i=0;i<count;i++)result.Add(Marshal.ReadIntPtr(data,8+i*IntPtr.Size).ToInt64());result.Sort();return result;
   }finally{Marshal.FreeHGlobal(data);}
  }
  uint Total(){Accounting value;if(!QueryAccounting(job,1,out value,(uint)Marshal.SizeOf(typeof(Accounting)),IntPtr.Zero))throw Error("job_accounting");status.totalAssignedProcesses=value.total;return value.total;}
  void Write(string row){
   byte[] bytes=new UTF8Encoding(false,true).GetBytes(row+"\n");
   if(bytes.Length>1<<20||status.bytes+bytes.Length>MaxBytes)throw new Exception("memory.evidence_budget");
   if(file==null||chunkBytes+bytes.Length>ChunkBytes){if(file!=null)file.Dispose();string path=prefix+".part"+status.chunks.Count+".jsonl";file=new FileStream(path,FileMode.CreateNew,FileAccess.Write,FileShare.Read);status.chunks.Add(path);chunkBytes=0;}
   file.Write(bytes,0,bytes.Length);file.Flush();chunkBytes+=bytes.Length;status.bytes+=bytes.Length;
  }
  void Sample(){
   long start=Stopwatch.GetTimestamp();string before=null,after=null,error=null;bool complete=true;
   ulong workingSet=0,privateBytes=0;var rows=new List<string>();var handles=new List<IntPtr>();var members=new List<long>();uint assigned=0;
   try{
    before=Phase();assigned=Total();members=Members();
    foreach(long pid in members){
     IntPtr handle=IntPtr.Zero;string image=null,creation=null,issue=null;Counters memory=new Counters{size=(uint)Marshal.SizeOf(typeof(Counters))};bool member=false;
     try{
      if(pid<=0||pid>UInt32.MaxValue)throw new Exception("memory.invalid_pid");
      handle=OpenProcess(QueryProcess,false,(uint)pid);if(handle==IntPtr.Zero)throw Error("open_owned_member");handles.Add(handle);
      FT created,exit,kernel,user;
      if(!IsProcessInJob(handle,job,out member)||!member||!GetProcessTimes(handle,out created,out exit,out kernel,out user))throw Error("member_identity");
      long identity=((long)created.high<<32)|created.low;creation=N(identity);
      if(identity<rootCreated||(pid==rootPid&&identity!=rootCreated))throw new Exception("memory.creation_identity");
      var path=new StringBuilder(32768);uint length=32768;if(!QueryFullProcessImageNameW(handle,0,path,ref length))throw Error("member_image");image=path.ToString();
      if(WaitForSingleObject(handle,0)!=258)throw new Exception("memory.member_not_live");
      if(!K32GetProcessMemoryInfo(handle,ref memory,(uint)Marshal.SizeOf(typeof(Counters))))throw Error("process_counters");
      if(WaitForSingleObject(handle,0)!=258)throw new Exception("memory.member_exited_during_read");
      checked{workingSet+=memory.workingSet.ToUInt64();privateBytes+=memory.privateUsage.ToUInt64();}
     }catch(Exception failure){issue=failure.Message;complete=false;}
     rows.Add("{\"pid\":"+N(pid)+",\"creationFileTime\":"+Q(creation)+",\"image\":"+Q(image)+",\"jobMember\":"+(member?"true":"false")+",\"error\":"+Q(issue)+",\"workingSetBytes\":"+U(memory.workingSet.ToUInt64())+",\"privateBytes\":"+U(memory.privateUsage.ToUInt64())+",\"kernelPeakWorkingSetBytes\":"+U(memory.peakWorkingSet.ToUInt64())+",\"kernelPeakPrivateCommitBytes\":"+U(memory.peakPagefile.ToUInt64())+"}");
    }
    var final=Members();if(members.Count!=final.Count)complete=false;else for(int i=0;i<members.Count;i++)if(members[i]!=final[i])complete=false;
    if(assigned!=Total())complete=false;
    foreach(IntPtr handle in handles)if(WaitForSingleObject(handle,0)!=258)complete=false;
    after=Phase();if(before!=after)complete=false;
   }catch(Exception failure){error=failure.Message;complete=false;}
   finally{foreach(IntPtr handle in handles)if(!CloseHandle(handle)){closeFailed=true;complete=false;error="memory.close_member_handle";}}
   long end=Stopwatch.GetTimestamp();
   Write("{\"schema\":\"lycheedev.owned-memory-sample.v1\",\"index\":"+status.samples+",\"startedTicks\":"+Q(N(start))+",\"endedTicks\":"+Q(N(end))+",\"phaseBefore\":"+Q(before)+",\"phaseAfter\":"+Q(after)+",\"complete\":"+(complete?"true":"false")+",\"error\":"+Q(error)+",\"totalAssignedProcesses\":"+status.totalAssignedProcesses+",\"workingSetBytes\":"+(complete?U(workingSet):"null")+",\"privateBytes\":"+(complete?U(privateBytes):"null")+",\"members\":["+String.Join(",",rows)+"]}");
   status.samples++;if(complete)status.completeSamples++;
  }
  void Run(){
   try{
    do{if(status.samples>=MaxSamples){status.budgetExceeded=true;throw new Exception("memory.sample_budget");}Sample();}while(!stop.WaitOne(status.intervalMs));
   }catch(Exception failure){status.error=failure.ToString();if(status.error.Contains("budget"))status.budgetExceeded=true;}
   finally{
    try{Total();}catch(Exception failure){status.error=(status.error??"")+"\n"+failure.Message;}
    try{if(file!=null)file.Dispose();}catch(Exception failure){status.error=(status.error??"")+"\n"+failure.Message;}
    status.handlesClosed=CloseHandle(job)&&!closeFailed;
   }
  }
  public Status Stop(){stop.Set();status.threadStopped=thread.Join(5000);if(status.threadStopped)stop.Dispose();else status.error=(status.error??"")+"\nmemory.observer_join_unverified";return status;}
 }
}

'@
try{$result=[ToolkitOwnedProcess]::Run($binary,[string[]]$requestData.args,[string]$requestData.cwd,[string]$requestData.stdout,[string]$requestData.stderr,[int]$requestData.timeoutMs,[int]$requestData.orphanMs,[string[]]$admittedHelperPaths,[string]$requestData.memoryPrefix,[string]$requestData.memoryMarker,[string]$requestData.rootIdentity,[string]$requestData.runId)}finally{foreach($file in $heldToolFiles){$file.Dispose()}}
# FILETIME exceeds the JSON/JavaScript exact integer range. Preserve the native
# Int64 comparisons above, and emit decimal strings at every evidence boundary.
function Convert-ObservedProcesses($rows) {
 if($null -eq $rows){return $null}
 return ,@($rows | ForEach-Object {
  @{pid=$_.pid;creationFileTime=$_.creationFileTime.ToString([Globalization.CultureInfo]::InvariantCulture);image=$_.image;error=$_.error;exited=$_.exited;jobMember=$_.jobMember;reviewedAuxiliary=$_.reviewedAuxiliary}
 })
}
$receipt=@{schema='lycheedev.owned-process.v1';runId=$requestData.runId;identitySource=$identitySource;startupIdentity=[string]$requestData.identity;rootIdentity=[string]$requestData.rootIdentity;binary=$binary;pid=$result.pid;creationFileTime=$result.creationFileTime.ToString([Globalization.CultureInfo]::InvariantCulture);exitCode=$result.exitCode;rootReaped=$result.rootReaped;treeReaped=$result.treeReaped;handlesClosed=$result.handlesClosed;toolIdentityLocksClosed=$true;outputReadersComplete=$result.outputReadersComplete;outputLimit=$result.outputLimit;activeProcesses=$result.activeProcesses;forced=$result.forced;knownAuxiliaryCleanup=$result.knownAuxiliaryCleanup;auxiliaryProcesses=(Convert-ObservedProcesses $result.auxiliaryProcesses);helperAdmission=$helperAdmission;deadlineHit=$result.deadlineHit;parentExitedBeforeChildren=$result.parentExitedBeforeChildren;drainCompletedByAccounting=$result.drainCompletedByAccounting;atDrainDeadline=(Convert-ObservedProcesses $result.atDrainDeadline);error=$result.error;memory=$result.memory;timing=@{schema="lycheedev.owned-timing.v1";frequency=[Diagnostics.Stopwatch]::Frequency;startTicks=$result.commandStartTicks.ToString([Globalization.CultureInfo]::InvariantCulture);rootExitTicks=$result.rootExitTicks.ToString([Globalization.CultureInfo]::InvariantCulture);treeReapedTicks=$result.treeReapedTicks.ToString([Globalization.CultureInfo]::InvariantCulture);boundary="before CreateProcessW through root/tree reap and output drain; excludes PowerShell startup and observer shutdown"}}
[IO.File]::WriteAllText([string]$requestData.receipt,($receipt|ConvertTo-Json -Depth 5))
if(!$result.treeReaped -or !$result.rootReaped -or !$result.handlesClosed -or !$result.outputReadersComplete){exit 8}
exit 0
