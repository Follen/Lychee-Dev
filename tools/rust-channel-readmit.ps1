param([Parameter(Mandatory=$true)][string]$Root,[Parameter(Mandatory=$true)][string]$Generation,[Parameter(Mandatory=$true)][string]$IntentSHA256,[Parameter(Mandatory=$true)][int]$CoordinatorPid)
$ErrorActionPreference='Stop'
$OwnerId='9f053173-8472-4860-be36-19a455a32e62';$BootFenceId='cc31783a-9ed6-47a8-a150-e656b88bd90e'
$authorization='PR14-user-authorized-stop-and-rerun-2026-10-16'
if($Generation -cnotmatch '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$' -or $IntentSHA256 -cnotmatch '^[0-9a-f]{64}$'){throw 'readmit.invalid_selector'}
$Root=[IO.Path]::GetFullPath($Root).TrimEnd('\')
if($Root -notmatch '^[A-Za-z]:\\' -or -not [IO.Directory]::Exists($Root)){throw 'readmit.local_repo_required'}
$held=New-Object 'System.Collections.Generic.List[System.IDisposable]'
$watch=$null;$self=$null;$coordinator=$null;$committed=$false;$primaryFailure=$null
function Local-Hash([byte[]]$bytes){$h=[Security.Cryptography.SHA256]::Create();try{return [BitConverter]::ToString($h.ComputeHash($bytes)).Replace('-','').ToLowerInvariant()}finally{$h.Dispose()}}
function Hold-Bytes([string]$path,[int]$maximum){$file=[IO.File]::Open($path,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::Read);$held.Add($file);if($file.Length -gt $maximum){throw 'readmit.file_budget'};$bytes=New-Object byte[] ([int]$file.Length);$offset=0;while($offset -lt $bytes.Length){$count=$file.Read($bytes,$offset,$bytes.Length-$offset);if($count -eq 0){throw 'readmit.short_read'};$offset+=$count};return ,$bytes}
try{
 $directory=Join-Path $Root ".tmp\rust-disk\recovery\$OwnerId\$BootFenceId"
 $attempt=Join-Path $directory "readmission\$Generation";$intentPath=Join-Path $attempt 'intent.json'
 $intentBytes=Hold-Bytes $intentPath 65536
 if((Local-Hash $intentBytes) -cne $IntentSHA256){throw 'readmit.intent_digest'}
 $intent=[Text.Encoding]::UTF8.GetString($intentBytes)|ConvertFrom-Json
 if($intent.schema -cne 'lycheedev.channel-readmission-intent.v1' -or $intent.ownerId -cne $OwnerId -or $intent.bootFenceId -cne $BootFenceId -or $intent.generation -cne $Generation -or $intent.authorization -cne $authorization -or $intent.preparationSHA256 -cnotmatch '^[0-9a-f]{64}$' -or $intent.oldOwnerHistoricalState -cne 'unknown' -or $intent.oldOwnerReaped -ne $false){throw 'readmit.intent_identity'}
 $preparedBytes=Hold-Bytes (Join-Path $directory 'prepared.json') 4194304
 if((Local-Hash $preparedBytes) -cne $intent.preparationSHA256){throw 'readmit.preparation_digest'}
 $prepared=[Text.Encoding]::UTF8.GetString($preparedBytes)|ConvertFrom-Json
 if($prepared.schema -cne 'lycheedev.channel-recovery-prepare.v1' -or $prepared.fenceId -cne $BootFenceId -or $prepared.snapshot.ownerId -cne $OwnerId -or $prepared.snapshot.channel -cne 'ordinary' -or $prepared.oldOwnerHistoricalState -cne 'unknown' -or $prepared.oldOwnerReaped -ne $false){throw 'readmit.preparation_target'}
 # Reuse the exact pinned original native type/functions, without running probe/commit.
 $legacyBytes=Hold-Bytes (Join-Path $PSScriptRoot 'rust-channel-recovery.ps1') 1048576
 if((Local-Hash $legacyBytes) -cne $prepared.toolHashes.powershell -or (Local-Hash (Hold-Bytes (Join-Path $PSScriptRoot 'rust-channel-recovery.mjs') 1048576)) -cne $prepared.toolHashes.javascript){throw 'readmit.original_boot_tools_changed'}
 if((Local-Hash (Hold-Bytes $PSCommandPath 1048576)) -cne $intent.tools.powershell -or (Local-Hash (Hold-Bytes (Join-Path $PSScriptRoot 'rust-channel-readmit.mjs') 1048576)) -cne $intent.tools.javascript){throw 'readmit.tools_changed'}
 $legacy=[Text.Encoding]::UTF8.GetString($legacyBytes)
 $native=[regex]::Matches($legacy,"(?s)Add-Type -TypeDefinition @'\r?\n(.*?)\r?\n'@")
 if($native.Count -ne 1){throw 'readmit.legacy_native_shape'}
 Add-Type -TypeDefinition $native[0].Groups[1].Value
 $tokens=$null;$parseErrors=$null;$ast=[Management.Automation.Language.Parser]::ParseInput($legacy,[ref]$tokens,[ref]$parseErrors)
 if($parseErrors.Count -ne 0){throw 'readmit.legacy_parse'}
 foreach($name in @('Hash-Bytes','Read-Boot','Assert-NoReparse','Assert-Same','Write-NewFlushed')){
  $nodes=@($ast.FindAll({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst]},$false)|Where-Object {$_.Name -ceq $name})
  if($nodes.Count -ne 1){throw 'readmit.legacy_function_shape'}
  . ([ScriptBlock]::Create($nodes[0].Extent.Text))
 }
 foreach($path in @($Root,$intentPath,(Join-Path $directory 'prepared.json'),$PSScriptRoot)){Assert-NoReparse $path}
 Assert-Same $prepared.root $Root
 $archive=Join-Path $attempt 'ordinary.lease.original';$recordPath=Join-Path $attempt 'admission.json';$target=Join-Path $Root 'target'
 Assert-Same $intent.archive $archive;Assert-Same $intent.target $target
 foreach($path in @($archive,$recordPath,$target)){Assert-NoReparse $path}
 if([IO.File]::Exists($archive) -or [IO.File]::Exists($recordPath) -or [IO.File]::Exists((Join-Path $directory 'ordinary.lease.original'))){throw 'readmit.inspect_existing_commit'}
 # BEGIN READMIT NATIVE: observation only; no termination or OS setting changes.
 Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;
public static class ToolkitReadmit {
 [StructLayout(LayoutKind.Sequential)] public struct FT {public uint low,high;}
 [StructLayout(LayoutKind.Sequential)] public struct UniqueProcess {public uint pid;public FT started;}
 [StructLayout(LayoutKind.Sequential,CharSet=CharSet.Unicode)] struct Info {
  public UniqueProcess process;
  [MarshalAs(UnmanagedType.ByValTStr,SizeConst=256)]public string app;
  [MarshalAs(UnmanagedType.ByValTStr,SizeConst=64)]public string service;
  public uint type,status,session;
  [MarshalAs(UnmanagedType.Bool)]public bool restartable;
 }
 public sealed class Identity {public uint pid;public string creationFileTime;}
 [DllImport("rstrtmgr.dll",CharSet=CharSet.Unicode)] static extern int RmStartSession(out uint handle,uint flags,StringBuilder key);
 [DllImport("rstrtmgr.dll",CharSet=CharSet.Unicode)] static extern int RmRegisterResources(uint handle,uint count,string[] files,uint processes,IntPtr applications,uint services,IntPtr serviceNames);
 [DllImport("rstrtmgr.dll")] static extern int RmGetList(uint handle,out uint needed,ref uint count,[In,Out]Info[] info,out uint reasons);
 [DllImport("rstrtmgr.dll")] static extern int RmEndSession(uint handle);
 public static Identity[] ResourceUsers(string[] paths){
  uint session;int result=RmStartSession(out session,0,new StringBuilder(33));if(result!=0)throw new Win32Exception(result,"readmit.rm_start");
  try{
   result=RmRegisterResources(session,(uint)paths.Length,paths,0,IntPtr.Zero,0,IntPtr.Zero);if(result!=0)throw new Win32Exception(result,"readmit.rm_register");
   uint needed,count=0,reasons;Info[] info=null;
   for(int attempt=0;attempt<3;attempt++){
    result=RmGetList(session,out needed,ref count,info,out reasons);
    if(result==234){if(needed>256)throw new Exception("readmit.rm_budget");info=new Info[needed];count=needed;continue;}
    if(result!=0)throw new Win32Exception(result,"readmit.rm_list");
    var rows=new Identity[count];for(int i=0;i<count;i++)rows[i]=new Identity{pid=info[i].process.pid,creationFileTime=(((ulong)info[i].process.started.high<<32)|info[i].process.started.low).ToString(System.Globalization.CultureInfo.InvariantCulture)};return rows;
   }
   throw new Exception("readmit.rm_changing");
  }finally{result=RmEndSession(session);if(result!=0)throw new Win32Exception(result,"readmit.rm_end");}
 }
 public sealed class Watch:IDisposable {
  readonly FileSystemWatcher watcher;int events,errors;
  public int Events{get{return Volatile.Read(ref events);}} public int Errors{get{return Volatile.Read(ref errors);}}
  public Watch(string path){watcher=new FileSystemWatcher(path){IncludeSubdirectories=true,NotifyFilter=NotifyFilters.FileName|NotifyFilters.DirectoryName|NotifyFilters.LastWrite|NotifyFilters.Size};watcher.Changed+=Changed;watcher.Created+=Changed;watcher.Deleted+=Changed;watcher.Renamed+=(s,e)=>Interlocked.Increment(ref events);watcher.Error+=(s,e)=>Interlocked.Increment(ref errors);watcher.EnableRaisingEvents=true;}
  void Changed(object sender,FileSystemEventArgs args){Interlocked.Increment(ref events);}
  public void Dispose(){watcher.Dispose();}
 }
}
'@
 # END READMIT NATIVE
 $rows=@($prepared.snapshot.files)
 if($rows.Count -ne 3 -or $rows[0].path -cne '.tmp/rust-disk/channels/ordinary.lease' -or $rows[1].path -cne ".tmp/rust-disk/evidence/$OwnerId/receipt.json" -or $rows[2].path -cne ".tmp/rust-disk/evidence/$OwnerId/process-11299fbf-7a09-4dc8-ba5d-e440145136f6.request.json"){throw 'readmit.snapshot_paths'}
 $nativeRelative=$rows[2].path -creplace '\.request\.json$','.receipt.json';$nativeReceipt=Join-Path $Root $nativeRelative
 if($prepared.snapshot.absentNativeReceipt -cne $nativeRelative){throw 'readmit.native_receipt_path'}
 $decoded=@();$originalLocks=@();$originalPaths=@()
 for($i=0;$i -lt 3;$i++){
  $row=$rows[$i];$path=Join-Path $Root $row.path;Assert-NoReparse $path
  $lock=New-Object ToolkitChannelFence+LockedFile $path,($i -eq 0);$held.Add($lock);$originalLocks+=@($lock);$originalPaths+=@($path)
  $bytes=$lock.Read(1048576)
  if($bytes.Length -ne $row.bytes -or (Hash-Bytes $bytes) -cne $row.sha256 -or [Convert]::ToBase64String($bytes) -cne $row.base64){throw 'readmit.original_evidence_changed'}
  $decoded+=@([Text.Encoding]::UTF8.GetString($bytes)|ConvertFrom-Json)
 }
 $lease=$decoded[0];$owner=$decoded[1];$request=$decoded[2]
 if($lease.schema -cne 'lycheedev.build-lease.v1' -or $lease.channel -cne 'ordinary' -or $lease.state -cne 'active' -or $lease.token -cne $intent.originalLeaseToken -or $rows[0].sha256 -cne $intent.originalLeaseSHA256 -or $owner.schema -cne 'lycheedev.disk-owner.v1' -or $owner.id -cne $OwnerId -or $owner.owner -cne $lease.owner -or @('active','interrupted') -cnotcontains $owner.state -or $owner.handlesReaped -eq $true -or ($owner.state -eq 'active' -and $owner.activePid -ne $lease.pid) -or $request.command -cne 'cargo'){throw 'readmit.owner_mismatch'}
 function Assert-EvidenceSet {
  $names=@(Get-ChildItem -LiteralPath (Join-Path $Root ".tmp\rust-disk\evidence\$OwnerId") -File -Filter 'process-*.request.json'|ForEach-Object {$_.Name})
  if($names.Count -ne 1 -or $names[0] -cne [IO.Path]::GetFileName($rows[2].path) -or [IO.File]::Exists($nativeReceipt) -or [IO.Directory]::Exists($nativeReceipt)){throw 'readmit.evidence_set_changed'}
 }
 Assert-EvidenceSet
 $resources=@('target/debug/.cargo-lock','target/debug/.cargo-build-lock','target/debug/.cargo-artifact-lock','target/release/.cargo-lock','target/release/.cargo-build-lock','target/release/.cargo-artifact-lock','target/debug/deps/lycheedev_data-8a8a1a4e2cb0a02b.exe','target/debug/deps/lycheedev_data-8a8a1a4e2cb0a02b.pdb','target/debug/lycheedev.exe','target/debug/lycheedev.pdb','target/.rustc_info.json','Cargo.lock')
 if(($intent.resourcePaths|ConvertTo-Json -Compress) -cne ($resources|ConvertTo-Json -Compress)){throw 'readmit.resource_set'}
 $dataIdentities=@(@{path='target/debug/deps/lycheedev_data-8a8a1a4e2cb0a02b.exe';bytes=63029248;sha256='5093d4a541a2b44fc4e7fbcfcabf28b7a7e8ead5e32210b5f29cc589b9274458'},@{path='target/debug/deps/lycheedev_data-8a8a1a4e2cb0a02b.pdb';bytes=139988992;sha256='e1c4e6f4c3f3f759dcafd8a665975fa1a6ae3b69b8a1396023d4c338f4846cdc'})
 $resourcePaths=@()
 foreach($relative in $resources){
  $path=Join-Path $Root $relative;Assert-NoReparse $path;$resourcePaths+=@($path)
  $file=[IO.File]::Open($path,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::None);$held.Add($file)
  foreach($expected in $dataIdentities|Where-Object {$_.path -ceq $relative}){
   $hash=[Security.Cryptography.SHA256]::Create();try{$digest=[BitConverter]::ToString($hash.ComputeHash($file)).Replace('-','').ToLowerInvariant()}finally{$hash.Dispose();$file.Position=0}
   if($file.Length -ne $expected.bytes -or $digest -cne $expected.sha256){throw 'readmit.data_identity'}
  }
 }
 $self=[Diagnostics.Process]::GetCurrentProcess();[void]$self.Handle
 $coordinator=[Diagnostics.Process]::GetProcessById($CoordinatorPid);[void]$coordinator.Handle
 $selfCreated=$self.StartTime.ToUniversalTime().ToFileTimeUtc().ToString([Globalization.CultureInfo]::InvariantCulture)
 $coordinatorCreated=$coordinator.StartTime.ToUniversalTime().ToFileTimeUtc().ToString([Globalization.CultureInfo]::InvariantCulture)
 $ownRow=Get-CimInstance -ClassName Win32_Process -Filter "ProcessId=$PID" -ErrorAction Stop
 if($ownRow.ParentProcessId -ne $CoordinatorPid){throw 'readmit.coordinator_identity'}
 function Assert-Coordinator {if($self.HasExited -or $coordinator.HasExited){throw 'readmit.coordinator_exited'}}
 function Related-Processes {
  Assert-Coordinator
  $found=@()
  foreach($p in @(Get-CimInstance -ClassName Win32_Process -Property ProcessId,Name,ExecutablePath,CommandLine,CreationDate -ErrorAction Stop)){
   if($p.ProcessId -eq $PID -or $p.ProcessId -eq $CoordinatorPid){
    $live=[Diagnostics.Process]::GetProcessById([int]$p.ProcessId)
    try{$created=$live.StartTime.ToUniversalTime().ToFileTimeUtc().ToString([Globalization.CultureInfo]::InvariantCulture);if(($p.ProcessId -eq $PID -and $created -ceq $selfCreated) -or ($p.ProcessId -eq $CoordinatorPid -and $created -ceq $coordinatorCreated)){continue};throw 'readmit.exclusion_identity_changed'}finally{$live.Dispose()}
   }
   $image=[string]$p.ExecutablePath;$command=[string]$p.CommandLine
   $related=$p.Name -match '^(cargo|rustc|cl|link|vctip|lycheedev(?:[_-].*)?)\.exe$' -or $image.StartsWith($Root+'\',[StringComparison]::OrdinalIgnoreCase) -or $command.IndexOf($OwnerId,[StringComparison]::OrdinalIgnoreCase) -ge 0 -or $command.IndexOf([IO.Path]::GetFileNameWithoutExtension($rows[2].path),[StringComparison]::OrdinalIgnoreCase) -ge 0 -or $command.IndexOf($target,[StringComparison]::OrdinalIgnoreCase) -ge 0
   if($related){$process=[Diagnostics.Process]::GetProcessById([int]$p.ProcessId);try{$found+=@(@{pid=$process.Id;creationFileTime=$process.StartTime.ToUniversalTime().ToFileTimeUtc().ToString([Globalization.CultureInfo]::InvariantCulture);name=$p.Name;image=$image})}finally{$process.Dispose()}}
  }
  return ,$found
 }
 function Resource-Users {
  Assert-Coordinator
  return ,@([ToolkitReadmit]::ResourceUsers([string[]]($originalPaths+$resourcePaths))|Where-Object {-not (($_.pid -eq $PID -and $_.creationFileTime -ceq $selfCreated) -or ($_.pid -eq $CoordinatorPid -and $_.creationFileTime -ceq $coordinatorCreated))})
 }
 function Target-Snapshot {
  $queue=New-Object 'System.Collections.Generic.Queue[string]';$queue.Enqueue($target)
  $items=New-Object 'System.Collections.Generic.List[string]'
  while($queue.Count -gt 0){$dir=$queue.Dequeue();Assert-NoReparse $dir
   foreach($path in [IO.Directory]::GetDirectories($dir)){Assert-NoReparse $path;$queue.Enqueue($path)}
   foreach($path in [IO.Directory]::GetFiles($dir)){Assert-NoReparse $path;$info=New-Object IO.FileInfo $path;$items.Add(($path.Substring($target.Length+1).Replace('\','/')+'|'+$info.Length.ToString([Globalization.CultureInfo]::InvariantCulture)+'|'+$info.LastWriteTimeUtc.Ticks.ToString([Globalization.CultureInfo]::InvariantCulture)));if($items.Count -gt 500000){throw 'readmit.target_budget'}}
  }
  $items.Sort([StringComparer]::Ordinal);return @{files=$items.Count;sha256=(Hash-Bytes ([Text.Encoding]::UTF8.GetBytes([string]::Join("`n",$items))))}
 }
 $beforeBoot=Read-Boot
 if($beforeBoot.machineSHA256 -cne $prepared.boot.machineSHA256 -or $beforeBoot.machineSHA256 -cne $intent.machineSHA256){throw 'readmit.different_machine'}
 $watch=New-Object ToolkitReadmit+Watch $target;$clock=[Diagnostics.Stopwatch]::StartNew()
 $relatedBefore=Related-Processes;$rmBefore=Resource-Users
 if($relatedBefore.Count -ne 0 -or $rmBefore.Count -ne 0){throw 'readmit.current_process_or_resource_busy'}
 $targetBefore=Target-Snapshot
 Start-Sleep -Milliseconds 10000
 $targetAfter=Target-Snapshot;$relatedAfter=Related-Processes;$rmAfter=Resource-Users
 if($relatedAfter.Count -ne 0 -or $rmAfter.Count -ne 0){throw 'readmit.current_process_or_resource_busy'}
 if($watch.Events -ne 0 -or $watch.Errors -ne 0 -or $targetBefore.sha256 -cne $targetAfter.sha256 -or $targetBefore.files -ne $targetAfter.files){throw 'readmit.writer_observed'}
 $afterBoot=Read-Boot
 if($afterBoot.machineSHA256 -cne $beforeBoot.machineSHA256 -or [UInt64]$afterBoot.uptime100ns -lt [UInt64]$beforeBoot.uptime100ns){throw 'readmit.host_or_boot_changed'}
 Assert-EvidenceSet;Assert-Coordinator
 $observation=@{schema='lycheedev.channel-live-idle.v1';originalFilesLocked=3;locksHeldThroughCommit=$true;resourcePaths=$resources;dataIdentities=$dataIdentities;lockedResources=$resources.Count;failedResources=0;relatedProcessesBefore=$relatedBefore;relatedProcessesAfter=$relatedAfter;restartManagerBefore=$rmBefore;restartManagerAfter=$rmAfter;watcherEvents=$watch.Events;watcherErrors=$watch.Errors;observationMilliseconds=$clock.ElapsedMilliseconds;targetBefore=$targetBefore;targetAfter=$targetAfter;beforeBoot=$beforeBoot;afterBoot=$afterBoot;exclusions=@(@{pid=$PID;creationFileTime=$selfCreated},@{pid=$CoordinatorPid;creationFileTime=$coordinatorCreated})}
 $record=@{schema='lycheedev.channel-readmission.v1';state='prepared';mode='user-authorized-stop-current-idle';ownerId=$OwnerId;bootFenceId=$BootFenceId;generation=$Generation;preparationSHA256=$intent.preparationSHA256;originalLeaseSHA256=$intent.originalLeaseSHA256;originalLeaseToken=$lease.token;authorization=$authorization;archive=$archive;target=$target;channel='ordinary';oldOwnerHistoricalState='unknown';oldOwnerReaped=$false;originalOwnerReceiptUnchanged=$true;scratchPreserved=$true;observation=$observation;commitFact='Exact original lease exists at archive after no-replace rename of the still-held original handle; this record alone is not commit';scope='Current cooperative Cargo/test resource exclusion for one explicitly authorized readmission. No historical Job/handle/output proof and no claim about arbitrary malicious writers.'}
 Write-NewFlushed $recordPath ([Text.Encoding]::UTF8.GetBytes(($record|ConvertTo-Json -Depth 10)))
 # Keep all original files and Cargo/resource locks held across the sole commit.
 Assert-EvidenceSet;Assert-Coordinator
 if((Related-Processes).Count -ne 0 -or (Resource-Users).Count -ne 0){throw 'readmit.process_or_resource_before_commit'}
 if($watch.Events -ne 0 -or $watch.Errors -ne 0){throw 'readmit.writer_before_commit'}
 $originalLocks[0].RenameNoReplace($archive);$committed=$true
 @{state='readmission_committed';generation=$Generation;archive=$archive;admission=$recordPath;oldOwnerReaped=$false}|ConvertTo-Json -Compress
}catch{$primaryFailure=$_;throw}finally{
 $cleanupErrors=New-Object 'System.Collections.Generic.List[string]'
 foreach($item in @($watch,$coordinator,$self)){if($item){try{$item.Dispose()}catch{$cleanupErrors.Add($_.Exception.Message)}}}
 for($i=$held.Count-1;$i -ge 0;$i--){try{$held[$i].Dispose()}catch{$cleanupErrors.Add($_.Exception.Message)}}
 if($cleanupErrors.Count -gt 0){
  $cleanup=@{state='cleanup_incomplete';generation=$Generation;renameReturnedSuccess=$committed;primaryError=if($primaryFailure){$primaryFailure.Exception.Message}else{$null};cleanupErrors=$cleanupErrors.ToArray();instruction='Inspect the exact archive; a cleanup failure does not roll back a committed rename.'}
  [Console]::Error.WriteLine(($cleanup|ConvertTo-Json -Depth 4 -Compress))
  if(-not $primaryFailure){throw 'readmit.cleanup_incomplete; inspect exact archive'}
 }
}
