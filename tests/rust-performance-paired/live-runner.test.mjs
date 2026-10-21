// Synthetic parser/negative tests only. These values are never performance evidence.
import assert from 'node:assert/strict';
import test from 'node:test';
import {readFileSync, mkdtempSync, rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join, resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import {RESULT_STRING_BYTES, HISTORY_EVENTS, REPEATS, SCOPES, APPEND_PHASES, verifyPerformanceReport} from './live-contract.mjs';
import {ORACLE} from '../rust-parity/formal-lua-3.2.4/assertions.mjs';
import {TEST, SUPPORT, performancePaths, selectTestExecutable, verifyTestOutput} from '../../tools/rust-live-offline-performance.mjs';
const root=fileURLToPath(new URL('../..',import.meta.url));
const clone=value=>structuredClone(value);
function fixture() {
  const samples=[];
  for(let repeat=0;repeat<REPEATS;repeat++) for(const history of HISTORY_EVENTS) for(const bytes of RESULT_STRING_BYTES) {
    const body=bytes+100,scopes=SCOPES.map(name=>({name,wallNanos:10}));
    samples.push({schema:'lycheedev.live-offline-sample.v1',repeat,input:{resultStringBytes:bytes,historyEvents:history},
      scopes,sampleWallNanos:SCOPES.length*10+5,unattributedWallNanos:5,
      completion:{bodyPanicked:false,peerCleanup:{Ok:null},journalCleanup:'Ok(())',verified:true},
      checks:Object.fromEntries(['exactReport','wireRoundtrip','checksumSequence','checkpointBoundary','persistBeforeRelease','noReplay','sixOrderedInputs','ownedReportPointerCapacity','ownedBorrowedExactWire','malformedCodecRejected'].map(key=>[key,true])),
      body:{requestedResultStringBytes:bytes,bodyPayloadBytes:body,bodySHA256:'a'.repeat(64)},
      history:{inputEvents:history,inputLogBytes:history*500,afterCheckpointEvents:history===256?1:history,afterCheckpointLogBytes:history===256?600:history*500,appendInputEvents:history===256?1:history},
      driverPartitionNanos:{journal:15,backendIncludingPeerExchange:15,driverAndMeasurementRemainder:30,total:60},
      nestedPeerExchange:{wallNanos:5,calls:6,includedIn:'driverPartitionNanos.backendIncludingPeerExchange',meaning:'not pure wait or game latency'},
      calls:{journal:{append:{calls:30,wallNanos:10},write_result:{calls:1,wallNanos:5}},backend:{publish:{calls:6,wallNanos:5},input:{calls:6,wallNanos:10}}},
      nestedPeerRecords:{readBytes:body+2000,decodeCalls:20,decodeWallNanos:3,payloadCopyBytes:body+1000,includedIn:'driverPartitionNanos.backendIncludingPeerExchange',scope:'not native process-memory IO'},
      ownedBytes:{snapshotArtifactCount:2,snapshotArtifactCopyBytes:body+99,snapshotArtifactCapacityBytes:body+99,codecPayloadCopyBytes:body+999,codecPayloadCapacityBytes:body+999,codecPayloadVectors:8,ownedProjectionReportCopyBytes:0,ownedProjectionReportCapacityBytes:body,borrowedProjectionReportCopyBytes:body,stateSerializedBytes:body+2000,terminalSerializedBytes:body+1000,totalAllocatorCalls:null,allocatorRequestedBytes:null,peakRSS:null}});
  }
  const sampleWallNanos=samples.reduce((sum,row)=>sum+row.sampleWallNanos,0);
  return {schema:'lycheedev.live-offline-performance.v1',oracle:ORACLE,state:'passed',samples,
    totalWallNanos:sampleWallNanos+20,authenticationWallNanos:10,sampleWallNanos,betweenSamplesAndEvidenceWallNanos:10,
    scope:'Rust-only; no game latency',timing:'Instant',memory:'capacities only'};
}
test('validates bounded 27-sample schema and nine diagnostic-only groups',()=>{
  const actual=verifyPerformanceReport(fixture());
  assert.equal(actual.samples,27);assert.equal(actual.groups.length,9);assert.equal(actual.acceptance,'diagnostic_only');
  assert.equal(actual.goHostComparison,'not_available');
});
function profiledFixture() {
  const report=fixture();
  for(const row of report.samples) row.nestedJournalAppend={
    schema:'lycheedev.live-journal-append-profile.v1', includedIn:'calls.journal.append.wallNanos',
    measurement:{calls:30,wallNanos:8,phases:Object.fromEntries(APPEND_PHASES.map(key=>[key,0])),
      cacheHits:29,fullChainReplays:1,readBytes:5000,equalityInputBytes:4000,replayedEvents:10,
      maximumCacheCapacityBytes:2000,maximumEligiblePairedRawCapacityBytes:4000},
    unattributedWallNanos:8,wrapperRemainderNanos:2,cacheCapacityLimitBytes:8<<20,eligiblePairedRawCapacityLimitBytes:16<<20,
    memoryScope:'eligible post-allocation raw cache/read capacity observations; excludes allocator excess/transient growth, event/State allocations and allocator/RSS totals'};
  return report;
}
test('accepts optional complete nested append profile without rewriting historical evidence',()=>{
  assert.equal(verifyPerformanceReport(profiledFixture()).samples,27);
  assert.equal(verifyPerformanceReport(fixture()).samples,27);
});
function ownershipFixture() {
  const report=profiledFixture();
  for(const row of report.samples) {
    row.nestedJournalAppend.measurement.snapshotOwnership={snapshotCalls:30,artifactEntries:60,
      borrowedEntries:59,borrowedBytes:1000,normalizedOwnedEntries:1,normalizedOwnedBytes:3,
      normalizedOwnedCapacityBytes:8,postNormalizationCopyBytes:0};
    row.nestedJournalAppend.snapshotOwnershipScope='Driver append final deduplicated artifact map only; capacity sums are observations; excludes normalization writes, discarded duplicate buffers, metadata and IO allocations; not allocator/RSS totals';
  }
  return report;
}
test('accepts complete snapshot ownership observations and both historical report generations',()=>{
  const borrowed=ownershipFixture();
  for(const row of borrowed.samples) Object.assign(row.nestedJournalAppend.measurement.snapshotOwnership,
    {borrowedEntries:60,normalizedOwnedEntries:0,normalizedOwnedBytes:0,normalizedOwnedCapacityBytes:0});
  for(const report of [borrowed,ownershipFixture(),profiledFixture(),fixture()]) assert.equal(verifyPerformanceReport(report).samples,27);
});
for(const [name,change] of [
  ['partial matrix',r=>delete r.samples[0].nestedJournalAppend.measurement.snapshotOwnership],
  ['setup calls',r=>r.samples[0].nestedJournalAppend.measurement.snapshotOwnership.snapshotCalls++],
  ['missing field',r=>delete r.samples[0].nestedJournalAppend.measurement.snapshotOwnership.borrowedBytes],
  ['entry mismatch',r=>r.samples[0].nestedJournalAppend.measurement.snapshotOwnership.borrowedEntries++],
  ['impossible artifact count',r=>r.samples[0].nestedJournalAppend.measurement.snapshotOwnership.artifactEntries=121],
  ['capacity below bytes',r=>r.samples[0].nestedJournalAppend.measurement.snapshotOwnership.normalizedOwnedCapacityBytes=2],
  ['false zero-copy',r=>r.samples[0].nestedJournalAppend.measurement.snapshotOwnership.postNormalizationCopyBytes=1],
  ['empty retained owner',r=>r.samples[0].nestedJournalAppend.measurement.snapshotOwnership.normalizedOwnedBytes=0],
  ['whole-heap claim',r=>r.samples[0].nestedJournalAppend.snapshotOwnershipScope='all allocator bytes'],
]) test(`rejects snapshot ownership ${name}`,()=>{const r=ownershipFixture();change(r);assert.throws(()=>verifyPerformanceReport(r));});
for(const [name,change] of [
  ['partial matrix',r=>delete r.samples[0].nestedJournalAppend],
  ['phase overlap',r=>r.samples[0].nestedJournalAppend.measurement.phases.read_chain=1],
  ['wrapper overlap',r=>r.samples[0].nestedJournalAppend.wrapperRemainderNanos++],
  ['setup calls included',r=>r.samples[0].nestedJournalAppend.measurement.calls++],
  ['unaccounted replay',r=>r.samples[0].nestedJournalAppend.measurement.fullChainReplays++],
  ['retained capacity excess',r=>r.samples[0].nestedJournalAppend.measurement.maximumCacheCapacityBytes=(8<<20)+1],
  ['paired capacity excess',r=>r.samples[0].nestedJournalAppend.measurement.maximumEligiblePairedRawCapacityBytes=(16<<20)+1],
  ['equality bytes exceed read',r=>r.samples[0].nestedJournalAppend.measurement.equalityInputBytes=5001],
  ['phase missing',r=>delete r.samples[0].nestedJournalAppend.measurement.phases.close_append],
]) test(`rejects append profile ${name}`,()=>{const r=profiledFixture();change(r);assert.throws(()=>verifyPerformanceReport(r));});
for(const [name,change] of [
  ['missing sample',r=>r.samples.pop()],['duplicate sample',r=>r.samples[1]=clone(r.samples[0])],
  ['wrong Go oracle',r=>r.oracle='0'.repeat(40)],['failure hidden',r=>r.samples[0].completion.bodyPanicked=true],
  ['missing full scope',r=>r.samples[0].scopes.pop()],['overlapping timing',r=>r.samples[0].unattributedWallNanos++],
  ['nested double count',r=>r.samples[0].driverPartitionNanos.total++],
  ['mock wait labeled as elapsed',r=>r.samples[0].nestedPeerExchange.wallNanos=1000],
  ['missing real body count',r=>r.samples[0].body.bodyPayloadBytes=undefined],
  ['same-input body altered',r=>r.samples[3].body.bodySHA256='b'.repeat(64)],
  ['checkpoint suppressed',r=>r.samples[6].history.afterCheckpointEvents=256],
  ['fake zero-copy',r=>r.samples[0].ownedBytes.ownedProjectionReportCopyBytes=1],
  ['fake allocator observation',r=>r.samples[0].ownedBytes.totalAllocatorCalls=100],
  ['missing serialization',r=>r.samples[0].ownedBytes.terminalSerializedBytes=0],
  ['NaN timer',r=>r.samples[0].calls.journal.append.wallNanos=NaN],
  ['cleanup unresolved',r=>r.samples[0].completion.peerCleanup={Err:'unjoined'}],
  ['ordering assertion absent',r=>delete r.samples[0].checks.sixOrderedInputs],
]) test(`rejects ${name}`,()=>{const report=fixture();change(report);assert.throws(()=>verifyPerformanceReport(report));});
const target=resolve('target'),artifact={reason:'compiler-artifact',target:{name:'lycheedev_live',kind:['lib']},profile:{test:true},executable:join(target,'release/deps/lycheedev_live-a1b2.exe')},complete={reason:'build-finished',success:true};
const jsonl=rows=>rows.map(row=>JSON.stringify(row)).join('\n');
test('selects only the exact named library test executable from Cargo',()=>assert.equal(selectTestExecutable(jsonl([artifact,complete]),target).executable,artifact.executable));
for(const [name,rows] of [
  ['missing',[complete]],['duplicate',[artifact,artifact,complete]],['failed build',[artifact,{...complete,success:false}]],
  ['non-test',[{...artifact,profile:{test:false}},complete]],['wrong target',[{...artifact,target:{name:'formal_lua_bridge',kind:['test']}},complete]],
  ['foreign executable',[{...artifact,executable:resolve('foreign/lycheedev_live-a1.exe')},complete]],
]) test(`rejects ${name} Cargo artifact`,()=>assert.throws(()=>selectTestExecutable(jsonl(rows),target)));
const output=`running 1 test\ntest ${TEST} ... ok\n\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 359 filtered out; finished in 1s\n`;
test('requires one exact explicitly selected completed benchmark test',()=>assert.equal(verifyTestOutput(Buffer.from(output)).passed,1));
for(const [name,text] of [['ignored',output.replace('0 ignored','1 ignored')],['wrong name',output.replace(TEST,'foreign')],['empty',output.replace(`test ${TEST} ... ok`,'')],['duplicate summary',output+output]])test(`rejects ${name} test result`,()=>assert.throws(()=>verifyTestOutput(Buffer.from(text))));
test('paths accept sealed records only and do not select a bare executable',()=>{
  const directory=mkdtempSync(join(tmpdir(),'live-offline-parser-'));
  try{
    assert.equal(performancePaths(['runtime/build-report.json','output'],directory).runtimePath,join(directory,'runtime/build-report.json'));
    assert.throws(()=>performancePaths(['runtime/lua.exe','output'],directory));
    assert.throws(()=>performancePaths(['runtime/build-report.json','candidate/artifact.json','output'],directory));
    assert.throws(()=>performancePaths(['../runtime/build-report.json','output'],directory));
  }finally{rmSync(directory,{recursive:true,force:true});}
});
test('benchmark remains ignored, monotonic, bounded, and production-hook-free',()=>{
  const source=readFileSync(join(root,'crates/lycheedev-live/src/go_mailbox_v1/connection/offline_performance_tests.rs'),'utf8');
  assert.match(source,/#\[ignore = "opt-in owned offline performance runner/);
  assert.match(source,/Instant::now\(\)/);assert.match(source,/Duration::from_secs\(20\)/);
  assert.match(source,/assert_eq!\(samples.len\(\), 27\)/);
  assert(!/unsafe\s*\{|thread::sleep|SystemTime|std::process::Command/.test(source));
  const peer=readFileSync(join(root,'crates/lycheedev-live/tests/support/formal_lua_peer.rs'),'utf8');
  assert.match(peer,/performance_bytes: None/);assert.match(peer,/PERFORMANCE_RESULT_STRING_BYTES.contains\(&bytes\)/);
  assert.match(peer,/fixture::performance_code\(bytes\).as_bytes\(\)/);
  const fixtureSource=readFileSync(join(root,'crates/lycheedev-live/tests/support/formal_lua_fixture.rs'),'utf8');
  assert.match(fixtureSource,/PERFORMANCE_RESULT_STRING_BYTES: \[usize; 3\] = \[256, 16384, 131072\]/);
  assert.match(fixtureSource,/PERFORMANCE_HISTORY_EVENTS: \[usize; 3\] = \[1, 255, 256\]/);
  assert(SUPPORT.includes('tests/rust-performance-paired/live-contract.mjs'));
  const runner=readFileSync(join(root,'tools/rust-live-offline-performance.mjs'),'utf8');
  assert.match(runner,/runCargo\(/);assert.match(runner,/ownedSpawnSync\(/);assert.match(runner,/'--release'/);
  assert.match(runner,/timeout: 690000/);assert.match(runner,/if \(succeeded && handlesReaped\) job.cleanup\(\)/);
});
