param([Parameter(Mandatory=$true)][string]$Plan,[Parameter(Mandatory=$true)][string]$PlanSHA256)
$ErrorActionPreference='Stop'
Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;
using Microsoft.Win32.SafeHandles;
public static class ToolkitOwnedRecovery {
 [StructLayout(LayoutKind.Sequential)] struct FT {public uint low,high;}
 [StructLayout(LayoutKind.Sequential)] struct Accounting {public long user,kernel,periodUser,periodKernel;public uint faults,total,active,terminated;}
 [StructLayout(LayoutKind.Sequential)] struct Basic {public long processTime,jobTime;public uint flags;public UIntPtr min,max;public uint active;public UIntPtr affinity;public uint priority,scheduling;}
 [StructLayout(LayoutKind.Sequential)] struct Io {public ulong reads,writes,others,readBytes,writeBytes,otherBytes;}
 [StructLayout(LayoutKind.Sequential)] struct Limits {public Basic basic;public Io io;public UIntPtr processMemory,jobMemory,peakProcess,peakJob;}
 [StructLayout(LayoutKind.Sequential,CharSet=CharSet.Unicode)] struct RenameInfo {public uint replace;public IntPtr root;public uint length;public char first;}
 [DllImport("kernel32.dll",SetLastError=true)] static extern IntPtr OpenProcess(uint access,bool inherit,uint pid);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool GetProcessTimes(IntPtr process,out FT creation,out FT exit,out FT kernel,out FT user);
 [DllImport("kernel32.dll",SetLastError=true)] static extern uint WaitForSingleObject(IntPtr handle,uint milliseconds);
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern IntPtr OpenJobObjectW(uint access,bool inherit,string name);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool TerminateJobObject(IntPtr job,uint code);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool QueryInformationJobObject(IntPtr job,int kind,out Accounting value,uint size,IntPtr returned);
 [DllImport("kernel32.dll",SetLastError=true,EntryPoint="QueryInformationJobObject")] static extern bool QueryLimits(IntPtr job,int kind,out Limits value,uint size,IntPtr returned);
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern IntPtr CreateFileW(string path,uint access,uint share,IntPtr security,uint disposition,uint flags,IntPtr template);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool SetFileInformationByHandle(IntPtr file,int kind,IntPtr data,uint size);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool CloseHandle(IntPtr handle);
 static Exception Error(string step){return new Win32Exception(Marshal.GetLastWin32Error(),"owned_recovery."+step);}
 public static string Observe(uint pid,long created,int session,string jobName,bool jobCreated){
  if(Process.GetCurrentProcess().SessionId!=session)throw new Exception("owned_recovery.different_windows_session");
  IntPtr wrapper=OpenProcess(0x00101000,false,pid);
  if(wrapper==IntPtr.Zero){if(Marshal.GetLastWin32Error()!=87)throw Error("open_wrapper");}
  else try{
   FT creation,exit,kernel,user;if(!GetProcessTimes(wrapper,out creation,out exit,out kernel,out user))throw Error("wrapper_identity");
   long actual=((long)creation.high<<32)|creation.low;
   // A reused PID is observed only. It never authorizes termination of that process.
   if(actual==created&&WaitForSingleObject(wrapper,0)!=0)throw new Exception("owned_recovery.wrapper_still_running");
  }finally{if(!CloseHandle(wrapper))throw Error("close_wrapper");}
  // The runner publishes the job identity before CreateProcessW. With no job
  // identity and an exited wrapper, no owned root could have been created.
  if(!jobCreated)return "no_root_created_before_job_identity";
  IntPtr job=OpenJobObjectW(0x000c,false,jobName);
  if(job==IntPtr.Zero){if(Marshal.GetLastWin32Error()==2)return "absent";throw Error("open_job");}
  try{
   Limits limits;if(!QueryLimits(job,9,out limits,(uint)Marshal.SizeOf(typeof(Limits)),IntPtr.Zero))throw Error("job_limits");
   if(limits.basic.flags!=0x2000)throw new Exception("owned_recovery.job_limits_changed");
   if(!TerminateJobObject(job,8))throw Error("terminate_job");
   var clock=Stopwatch.StartNew();
   for(;;){Accounting accounting;if(!QueryInformationJobObject(job,1,out accounting,(uint)Marshal.SizeOf(typeof(Accounting)),IntPtr.Zero))throw Error("job_accounting");if(accounting.active==0)return "terminated_active_zero";if(clock.ElapsedMilliseconds>=15000)throw new Exception("owned_recovery.tree_exit_unverified");Thread.Sleep(20);}
  }finally{if(!CloseHandle(job))throw Error("close_job");}
 }
 public sealed class LockedFile:IDisposable {
  IntPtr handle;FileStream stream;
  public LockedFile(string path,bool rename){handle=CreateFileW(path,0x80000000u|(rename?0x10000u:0u),1,IntPtr.Zero,3,0x00200000,IntPtr.Zero);if(handle==new IntPtr(-1))throw Error("lock_original");try{stream=new FileStream(new SafeFileHandle(handle,false),FileAccess.Read);}catch{CloseHandle(handle);handle=IntPtr.Zero;throw;}}
  public byte[] Read(){if(stream.Length>1048576)throw new Exception("owned_recovery.file_budget");stream.Position=0;var bytes=new byte[(int)stream.Length];int offset=0,count;while(offset<bytes.Length&&(count=stream.Read(bytes,offset,bytes.Length-offset))>0)offset+=count;if(offset!=bytes.Length)throw new Exception("owned_recovery.short_read");return bytes;}
  public void RenameNoReplace(string destination){byte[] name=Encoding.Unicode.GetBytes(destination);int offset=(int)Marshal.OffsetOf(typeof(RenameInfo),"first"),size=checked(offset+name.Length);IntPtr data=Marshal.AllocHGlobal(size);try{for(int i=0;i<size;i++)Marshal.WriteByte(data,i,0);Marshal.WriteInt32(data,(int)Marshal.OffsetOf(typeof(RenameInfo),"length"),name.Length);Marshal.Copy(name,0,new IntPtr(data.ToInt64()+offset),name.Length);if(!SetFileInformationByHandle(handle,3,data,(uint)size))throw Error("archive_commit");}finally{Marshal.FreeHGlobal(data);}}
  public void Dispose(){if(stream!=null){stream.Dispose();stream=null;}if(handle!=IntPtr.Zero){IntPtr closing=handle;handle=IntPtr.Zero;if(!CloseHandle(closing))throw Error("close_original");}}
 }
}
'@
function Hash-Bytes([byte[]]$bytes){$hash=[Security.Cryptography.SHA256]::Create();try{return [BitConverter]::ToString($hash.ComputeHash($bytes)).Replace('-','').ToLowerInvariant()}finally{$hash.Dispose()}}
function Assert-NoReparse([string]$path){$cursor=[IO.Path]::GetFullPath($path);while($cursor){if([IO.File]::Exists($cursor) -or [IO.Directory]::Exists($cursor)){if(([IO.File]::GetAttributes($cursor) -band [IO.FileAttributes]::ReparsePoint) -ne 0){throw 'owned_recovery.reparse_boundary'}};$cursor=[IO.Path]::GetDirectoryName($cursor)}}
function Assert-Same([string]$a,[string]$b){if(-not [string]::Equals([IO.Path]::GetFullPath($a),[IO.Path]::GetFullPath($b),[StringComparison]::OrdinalIgnoreCase)){throw 'owned_recovery.path_mismatch'}}
function Write-NewFlushed([string]$path,$value){$bytes=[Text.Encoding]::UTF8.GetBytes(($value|ConvertTo-Json -Depth 8));$temporary=$path+'.'+[Guid]::NewGuid().ToString('D')+'.writing';$file=New-Object IO.FileStream $temporary,([IO.FileMode]::CreateNew),([IO.FileAccess]::Write),([IO.FileShare]::Read),4096,([IO.FileOptions]::WriteThrough);try{$file.Write($bytes,0,$bytes.Length);$file.Flush($true)}finally{$file.Dispose()};[IO.File]::Move($temporary,$path)}
Assert-NoReparse $Plan
$planBytes=[IO.File]::ReadAllBytes($Plan)
if($planBytes.Length -gt 1048576 -or $PlanSHA256 -cnotmatch '^[0-9a-f]{64}$' -or (Hash-Bytes $planBytes) -cne $PlanSHA256){throw 'owned_recovery.plan_digest'}
$p=[Text.Encoding]::UTF8.GetString($planBytes)|ConvertFrom-Json
$uuid='^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
if($p.schema -cne 'lycheedev.owned-recovery-plan.v1' -or $p.ownerId -cnotmatch $uuid -or $p.runId -cnotmatch $uuid -or $p.channel -cne 'ordinary' -or $p.jobName -cne ('Local\LycheeDev.Owned.'+$p.runId)){throw 'owned_recovery.plan_identity'}
$root=[IO.Path]::GetFullPath($p.root).TrimEnd('\');Assert-NoReparse $root
$directory=Join-Path $root ('.tmp\rust-disk\recovery\'+$p.ownerId+'\'+$p.runId+'\same-boot')
Assert-Same ([IO.Path]::GetDirectoryName($Plan)) $directory
Assert-Same $Plan (Join-Path $directory ('plan-'+$PlanSHA256+'.json'))
$archive=Join-Path $directory 'ordinary.lease.original';$fencePath=Join-Path $directory 'fence.json';$leasePath=Join-Path $root '.tmp\rust-disk\channels\ordinary.lease'
$base=Join-Path $root ('.tmp\rust-disk\evidence\'+$p.ownerId+'\process-'+$p.runId)
$expected=@($leasePath,(Join-Path $root ('.tmp\rust-disk\evidence\'+$p.ownerId+'\receipt.json')),($base+'.request.json'),($base+'.identity.json'))
if($p.jobCreated){$expected+=@($base+'.job.json')};if($p.rootRecorded){$expected+=@($base+'.root.json')};if($p.originalTerminalReceiptPresent){$expected+=@($base+'.receipt.json')}
$rows=@($p.files);if($rows.Count -ne $expected.Count){throw 'owned_recovery.snapshot_shape'}
for($i=0;$i -lt $rows.Count;$i++){Assert-Same $rows[$i].path $expected[$i];Assert-NoReparse $expected[$i]}
Assert-NoReparse $archive;Assert-NoReparse $fencePath
function Read-Fence {
 $bytes=[IO.File]::ReadAllBytes($fencePath);if($bytes.Length -gt 65536){throw 'owned_recovery.fence_budget'};$value=[Text.Encoding]::UTF8.GetString($bytes)|ConvertFrom-Json
 if($value.schema -cne 'lycheedev.owned-recovery.v1' -or $value.state -cne 'channel_released' -or $value.ownerId -cne $p.ownerId -or $value.runId -cne $p.runId -or $value.planSHA256 -cne $PlanSHA256 -or $value.originalLeaseSHA256 -cne $rows[0].sha256 -or $value.wrapperExited -ne $true -or $value.activeProcesses -ne 0 -or $value.originalTerminalReceiptCreated -ne $false -or $value.originalOwnerReceiptUnchanged -ne $true){throw 'owned_recovery.existing_fence_mismatch'}
 Assert-Same $value.archive $archive;Assert-Same $value.plan $Plan
 return $value
}
$held=New-Object 'System.Collections.Generic.List[System.IDisposable]'
try{
 $committed=[IO.File]::Exists($archive)
 for($i=0;$i -lt $rows.Count;$i++){
  $path=$expected[$i];if($i -eq 0 -and $committed){$path=$archive}
  $lock=New-Object ToolkitOwnedRecovery+LockedFile $path,($i -eq 0 -and -not $committed);$held.Add($lock)
  if((Hash-Bytes $lock.Read()) -cne $rows[$i].sha256){throw 'owned_recovery.original_evidence_changed'}
 }
 if($committed){$fence=Read-Fence;$fence.state='commit_observed';$fence|Add-Member -NotePropertyName currentLease -NotePropertyValue $(if([IO.File]::Exists($leasePath)){'present_do_not_modify'}else{'absent'}) -Force}
 else{
  if([IO.File]::Exists($fencePath)){$fence=Read-Fence}
  else{
   $observation=[ToolkitOwnedRecovery]::Observe([uint32]$p.wrapperPid,[int64]$p.wrapperCreationFileTime,[int]$p.wrapperSessionId,[string]$p.jobName,[bool]$p.jobCreated)
   # A sidecar published after Node's capture requires a fresh capture. Never
   # infer that the pre-CreateProcess window still holds from stale absence.
   if((-not $p.jobCreated -and [IO.File]::Exists($base+'.job.json')) -or (-not $p.rootRecorded -and [IO.File]::Exists($base+'.root.json')) -or (-not $p.originalTerminalReceiptPresent -and [IO.File]::Exists($base+'.receipt.json'))){throw 'owned_recovery.original_evidence_changed'}
   $fence=@{schema='lycheedev.owned-recovery.v1';state='channel_released';ownerId=$p.ownerId;runId=$p.runId;jobName=$p.jobName;wrapperExited=$true;jobObservation=$observation;activeProcesses=0;plan=$Plan;planSHA256=$PlanSHA256;originalLeaseSHA256=$rows[0].sha256;originalOwnerReceiptUnchanged=$true;originalTerminalReceiptCreated=$false;originalTerminalReceiptPresent=$p.originalTerminalReceiptPresent;scratchPreserved=$true;archive=$archive;fence=$fencePath;target=(Join-Path $root 'target');commitFact='Only the exact original lease archived by no-replace handle rename commits release.'}
   Write-NewFlushed $fencePath $fence
  }
  $held[0].RenameNoReplace($archive)
 }
}finally{for($i=$held.Count-1;$i -ge 0;$i--){$held[$i].Dispose()}}
$fence|ConvertTo-Json -Depth 8 -Compress
