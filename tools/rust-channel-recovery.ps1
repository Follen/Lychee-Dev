param([Parameter(Mandatory=$true)][ValidateSet('probe','commit')][string]$Action,[string]$Root,[string]$OwnerId,[string]$FenceId,[string]$PreparationSHA256)
$ErrorActionPreference='Stop'
Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using Microsoft.Win32.SafeHandles;
public static class ToolkitChannelFence {
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool QueryUnbiasedInterruptTime(out ulong value);
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern IntPtr CreateFileW(string path,uint access,uint share,IntPtr security,uint disposition,uint flags,IntPtr template);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool SetFileInformationByHandle(IntPtr file,int kind,IntPtr data,uint size);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool CloseHandle(IntPtr handle);
 [StructLayout(LayoutKind.Sequential,CharSet=CharSet.Unicode)] struct RenameInfo {public uint replace;public IntPtr root;public uint length;public char first;}
 public static ulong Counter(){ulong value;if(!QueryUnbiasedInterruptTime(out value))throw new Win32Exception(Marshal.GetLastWin32Error(),"recovery.boot_counter");return value;}
 public sealed class LockedFile:IDisposable {
  IntPtr handle;FileStream stream;
  public LockedFile(string path,bool rename){handle=CreateFileW(path,0x80000000u|(rename?0x10000u:0u),1,IntPtr.Zero,3,0x00200000,IntPtr.Zero);if(handle==new IntPtr(-1))throw new Win32Exception(Marshal.GetLastWin32Error(),"recovery.lock_original");try{stream=new FileStream(new SafeFileHandle(handle,false),FileAccess.Read);}catch{CloseHandle(handle);handle=IntPtr.Zero;throw;}}
  public byte[] Read(int maximum){if(stream.Length>maximum)throw new Exception("recovery.file_budget");stream.Position=0;var bytes=new byte[(int)stream.Length];int offset=0,count;while(offset<bytes.Length&&(count=stream.Read(bytes,offset,bytes.Length-offset))>0)offset+=count;if(offset!=bytes.Length)throw new Exception("recovery.short_read");return bytes;}
  public void RenameNoReplace(string destination){byte[] name=Encoding.Unicode.GetBytes(destination);int offset=(int)Marshal.OffsetOf(typeof(RenameInfo),"first"),size=checked(offset+name.Length);IntPtr data=Marshal.AllocHGlobal(size);try{for(int i=0;i<size;i++)Marshal.WriteByte(data,i,0);Marshal.WriteInt32(data,(int)Marshal.OffsetOf(typeof(RenameInfo),"length"),name.Length);Marshal.Copy(name,0,new IntPtr(data.ToInt64()+offset),name.Length);if(!SetFileInformationByHandle(handle,3,data,(uint)size))throw new Win32Exception(Marshal.GetLastWin32Error(),"recovery.archive_commit");}finally{Marshal.FreeHGlobal(data);}}
  public void Dispose(){if(stream!=null){stream.Dispose();stream=null;}if(handle!=IntPtr.Zero){IntPtr closing=handle;handle=IntPtr.Zero;if(!CloseHandle(closing))throw new Win32Exception(Marshal.GetLastWin32Error(),"recovery.close_original");}}
 }
}
'@
function Hash-Bytes([byte[]]$bytes){$hash=[Security.Cryptography.SHA256]::Create();try{return [BitConverter]::ToString($hash.ComputeHash($bytes)).Replace('-','').ToLowerInvariant()}finally{$hash.Dispose()}}
function Read-Boot {
 [UInt64]$before=[ToolkitChannelFence]::Counter()
 $uuid=[string](Get-CimInstance -ClassName Win32_ComputerSystemProduct -Property UUID -ErrorAction Stop).UUID
 if($uuid -notmatch '^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$' -or @('00000000-0000-0000-0000-000000000000','ffffffff-ffff-ffff-ffff-ffffffffffff','03000200-0400-0500-0006-000700080009') -contains $uuid.ToLowerInvariant()){throw 'recovery.machine_identity_unavailable'}
 [UInt64]$after=[ToolkitChannelFence]::Counter()
 if($after -lt $before -or $after -ge 9223372036854775808){throw 'recovery.boot_counter_invalid'}
 return @{schema='lycheedev.windows-boot-counter.v1';machineSHA256=(Hash-Bytes ([Text.Encoding]::UTF8.GetBytes($uuid.ToLowerInvariant())));uptime100ns=$after.ToString([Globalization.CultureInfo]::InvariantCulture)}
}
function Assert-NoReparse([string]$path){$cursor=[IO.Path]::GetFullPath($path);while($cursor){if([IO.File]::Exists($cursor) -or [IO.Directory]::Exists($cursor)){if(([IO.File]::GetAttributes($cursor) -band [IO.FileAttributes]::ReparsePoint) -ne 0){throw 'recovery.reparse_boundary'}};$cursor=[IO.Path]::GetDirectoryName($cursor)}}
function Assert-Same([string]$a,[string]$b){if(-not [string]::Equals($a,$b,[StringComparison]::OrdinalIgnoreCase)){throw 'recovery.target_mismatch'}}
function Assert-NewBoot($before,$after){if($before.schema -cne 'lycheedev.windows-boot-counter.v1' -or $after.schema -cne $before.schema -or $before.machineSHA256 -cne $after.machineSHA256 -or $before.uptime100ns -isnot [string] -or $after.uptime100ns -isnot [string] -or [string]$before.uptime100ns -notmatch '^(0|[1-9][0-9]*)$' -or [string]$after.uptime100ns -notmatch '^(0|[1-9][0-9]*)$'){throw 'recovery.boot_identity_invalid'};if([UInt64]$before.uptime100ns -ge 9223372036854775808 -or [UInt64]$after.uptime100ns -ge [UInt64]$before.uptime100ns){throw 'recovery.new_full_boot_unproven'}}
function Write-NewFlushed([string]$path,[byte[]]$bytes){$file=New-Object IO.FileStream $path,([IO.FileMode]::CreateNew),([IO.FileAccess]::Write),([IO.FileShare]::Read),4096,([IO.FileOptions]::WriteThrough);try{$file.Write($bytes,0,$bytes.Length);$file.Flush($true)}finally{$file.Dispose()}}
if($Action -eq 'probe'){Read-Boot|ConvertTo-Json -Compress;exit 0}
$uuidPattern='^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
if($OwnerId -cnotmatch $uuidPattern -or $FenceId -cnotmatch $uuidPattern -or $PreparationSHA256 -cnotmatch '^[0-9a-f]{64}$'){throw 'recovery.invalid_selector'}
$Root=[IO.Path]::GetFullPath($Root).TrimEnd('\')
if($Root -notmatch '^[A-Za-z]:\\' -or -not [IO.Directory]::Exists($Root)){throw 'recovery.local_repo_required'}
Assert-NoReparse $Root
$directory=Join-Path $Root ".tmp\rust-disk\recovery\$OwnerId\$FenceId"
$preparedPath=Join-Path $directory 'prepared.json';$archive=Join-Path $directory 'ordinary.lease.original';$fencePath=Join-Path $directory 'fence.json'
Assert-NoReparse $preparedPath
$preparedBytes=[IO.File]::ReadAllBytes($preparedPath)
if($preparedBytes.Length -gt 4194304 -or (Hash-Bytes $preparedBytes) -cne $PreparationSHA256){throw 'recovery.preparation_digest'}
$prepared=[Text.Encoding]::UTF8.GetString($preparedBytes)|ConvertFrom-Json
if($prepared.schema -cne 'lycheedev.channel-recovery-prepare.v1' -or $prepared.fenceId -cne $FenceId -or $prepared.snapshot.ownerId -cne $OwnerId -or $prepared.snapshot.channel -cne 'ordinary' -or $prepared.oldOwnerHistoricalState -cne 'unknown' -or $prepared.oldOwnerReaped -ne $false){throw 'recovery.preparation_target'}
Assert-Same $prepared.root $Root
if((Hash-Bytes ([IO.File]::ReadAllBytes($PSCommandPath))) -cne $prepared.toolHashes.powershell -or (Hash-Bytes ([IO.File]::ReadAllBytes([IO.Path]::Combine($PSScriptRoot,'rust-channel-recovery.mjs')))) -cne $prepared.toolHashes.javascript){throw 'recovery.tool_changed_since_prepare'}
$before=Read-Boot;Assert-NewBoot $prepared.boot $before
$rows=@($prepared.snapshot.files)
if($rows.Count -ne 3 -or $rows[0].path -cne '.tmp/rust-disk/channels/ordinary.lease' -or $rows[1].path -cne ".tmp/rust-disk/evidence/$OwnerId/receipt.json" -or $rows[2].path -cnotmatch "^\.tmp/rust-disk/evidence/$OwnerId/process-[0-9a-f-]{36}\.request\.json$"){throw 'recovery.snapshot_paths'}
$nativeRelative=$rows[2].path -creplace '\.request\.json$','.receipt.json'
if($prepared.snapshot.absentNativeReceipt -cne $nativeRelative){throw 'recovery.native_receipt_path'}
$nativeReceipt=Join-Path $Root $nativeRelative
if([IO.File]::Exists($nativeReceipt) -or [IO.Directory]::Exists($nativeReceipt)){throw 'recovery.native_receipt_exists'}
$held=New-Object 'System.Collections.Generic.List[System.IDisposable]'
try {
 $decoded=@()
 for($i=0;$i -lt 3;$i++){
  $row=$rows[$i];$path=Join-Path $Root $row.path;Assert-NoReparse $path
  $lock=New-Object ToolkitChannelFence+LockedFile $path,($i -eq 0);$held.Add($lock)
  $bytes=$lock.Read(1048576)
  if($bytes.Length -ne $row.bytes -or (Hash-Bytes $bytes) -cne $row.sha256 -or [Convert]::ToBase64String($bytes) -cne $row.base64){throw 'recovery.original_evidence_changed'}
  $decoded+=@([Text.Encoding]::UTF8.GetString($bytes)|ConvertFrom-Json)
 }
 $lease=$decoded[0];$owner=$decoded[1];$request=$decoded[2]
 if($lease.schema -cne 'lycheedev.build-lease.v1' -or $lease.channel -cne 'ordinary' -or $lease.state -cne 'active' -or $lease.token -cnotmatch $uuidPattern -or $owner.schema -cne 'lycheedev.disk-owner.v1' -or $owner.id -cne $OwnerId -or $owner.owner -cne $lease.owner -or @('active','interrupted') -cnotcontains $owner.state -or $owner.handlesReaped -eq $true -or ($owner.state -eq 'active' -and $owner.activePid -ne $lease.pid) -or $request.command -cne 'cargo'){throw 'recovery.owner_mismatch'}
 $requestNames=@(Get-ChildItem -LiteralPath (Join-Path $Root ".tmp\rust-disk\evidence\$OwnerId") -File -Filter 'process-*.request.json'|ForEach-Object {$_.Name})
 if($requestNames.Count -ne 1 -or $requestNames[0] -cne [IO.Path]::GetFileName($rows[2].path) -or [IO.File]::Exists($nativeReceipt) -or [IO.Directory]::Exists($nativeReceipt)){throw 'recovery.evidence_set_changed'}
 if([IO.File]::Exists($archive) -or [IO.Directory]::Exists($archive)){throw 'recovery.inspect_existing_archive_before_retry'}
 Assert-NoReparse $archive
 $after=Read-Boot;Assert-NewBoot $prepared.boot $after
 if($before.machineSHA256 -cne $after.machineSHA256 -or [UInt64]$after.uptime100ns -lt [UInt64]$before.uptime100ns){throw 'recovery.boot_changed_during_commit'}
 $fence=@{schema='lycheedev.channel-boot-fence.v1';state='prepared';commitFact='ordinary.lease.original exists with exact original SHA256; its no-replace handle rename is the only release commit';ownerId=$OwnerId;fenceId=$FenceId;preparationSHA256=$PreparationSHA256;originalLeaseSHA256=$rows[0].sha256;archive=$archive;channel='ordinary';target=(Join-Path $Root 'target');beforeBoot=$prepared.boot;afterBoot=$after;oldOwnerHistoricalState='unknown';oldOwnerReaped=$false;originalOwnerReceiptUnchanged=$true;scratchPreserved=$true}
 if([IO.File]::Exists($fencePath)){
  $oldFenceBytes=[IO.File]::ReadAllBytes($fencePath)
  if($oldFenceBytes.Length -gt 65536){throw 'recovery.fence_budget'}
  $oldFence=[Text.Encoding]::UTF8.GetString($oldFenceBytes)|ConvertFrom-Json
  if($oldFence.schema -cne $fence.schema -or $oldFence.state -cne 'prepared' -or $oldFence.ownerId -cne $OwnerId -or $oldFence.fenceId -cne $FenceId -or $oldFence.preparationSHA256 -cne $PreparationSHA256 -or $oldFence.originalLeaseSHA256 -cne $rows[0].sha256 -or $oldFence.oldOwnerReaped -ne $false){throw 'recovery.existing_fence_mismatch'}
  Assert-Same $oldFence.archive $archive
  Assert-Same $oldFence.target $fence.target
  if($oldFence.channel -cne 'ordinary' -or $oldFence.oldOwnerHistoricalState -cne 'unknown' -or $oldFence.originalOwnerReceiptUnchanged -ne $true -or $oldFence.scratchPreserved -ne $true -or $oldFence.commitFact -cne $fence.commitFact -or $oldFence.beforeBoot.schema -cne $prepared.boot.schema -or $oldFence.beforeBoot.machineSHA256 -cne $prepared.boot.machineSHA256 -or $oldFence.beforeBoot.uptime100ns -cne $prepared.boot.uptime100ns){throw 'recovery.existing_fence_mismatch'}
  Assert-NewBoot $prepared.boot $oldFence.afterBoot
 }else{Write-NewFlushed $fencePath ([Text.Encoding]::UTF8.GetBytes(($fence|ConvertTo-Json -Depth 8)))}
 # All proof and durable evidence precede this one atomic namespace operation.
 # A failed no-replace rename leaves the original lease. A lost response requires
 # inspection of the archive, never a guessed rollback or another owner deletion.
 $held[0].RenameNoReplace($archive)
} finally {for($i=$held.Count-1;$i -ge 0;$i--){$held[$i].Dispose()}}
@{state='channel_fenced_after_verified_counter_reset';ownerId=$OwnerId;fenceId=$FenceId;archive=$archive;fence=$fencePath;oldOwnerReaped=$false;target=(Join-Path $Root 'target');next='Use the unchanged ordinary-channel runner. Preserve old owner, scratch and evidence.'}|ConvertTo-Json -Compress
