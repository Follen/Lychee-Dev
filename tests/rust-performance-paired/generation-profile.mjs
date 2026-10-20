// New-product diagnostics are explicitly bound and cannot upgrade old evidence.
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import * as profiles from './data-phase-profile.mjs';
export const HISTORICAL_GENERATION_PRODUCT = Object.freeze({
  commit:'7bd4216c28b45761f27239e1d74536af71b11a9b',
  sourceTreeSHA256:'cbad32ff06cac1fbcd01cbba5dd6dc5a8028a70e4a8741cf2a44a1266cdeeadd',
});
const baseKeys=['schema','ownerRoot','artifact','goSource','goPackage','luaLSArchive','fixturePreparation'];
export function generationMode(request, mode) {
  assert(['--observation','--acceptance','--profile-observation'].includes(mode),'explicit generation mode');
  const profiled=mode==='--profile-observation';
  assert.deepEqual(Object.keys(request).sort(),[...baseKeys,...(profiled?['product']:[])].sort(),'exact generation request fields');
  assert.equal(request.schema,profiled?'lycheedev.data-generation-config.v2':'lycheedev.data-generation-config.v1');
  if(profiled){
    const p=request.product;
    assert.deepEqual(Object.keys(p).sort(),['commit','sourceTreeSHA256','binarySHA256','automaticEvictionEnabled'].sort());
    assert.match(p.commit,/^[0-9a-f]{40}$/);assert.match(p.sourceTreeSHA256,/^[0-9a-f]{64}$/);assert.match(p.binarySHA256,/^[0-9a-f]{64}$/);
    assert.equal(p.automaticEvictionEnabled,true,'new-product diagnostic requires explicit automatic eviction contract');
    assert.notEqual(p.commit,HISTORICAL_GENERATION_PRODUCT.commit,'historical product must retain original false contract');
    assert.notEqual(p.sourceTreeSHA256,HISTORICAL_GENERATION_PRODUCT.sourceTreeSHA256,'old product bytes cannot be relabeled');
  }
  return {profiled,acceptance:mode==='--acceptance',product:profiled?structuredClone(request.product):null};
}
export function verifyGenerationProfileProduct(binding, artifact, commit, status) {
  assert.equal(status,'','exact frozen product requires clean tracked and untracked source state');
  assert(binding&&binding.automaticEvictionEnabled===true);
  assert.equal(commit,binding.commit,'exact frozen new-product checkout commit');
  assert.equal(artifact.inputHashes.sourceTree,binding.sourceTreeSHA256,'bound new-product source bytes');
  assert.equal(artifact.binarySHA256,binding.binarySHA256,'bound new-product executable bytes');
  assert.notEqual(commit,HISTORICAL_GENERATION_PRODUCT.commit);assert.notEqual(binding.sourceTreeSHA256,HISTORICAL_GENERATION_PRODUCT.sourceTreeSHA256);
  return true;
}
export function generationProfileEnvironment(environment,{enabled=false,measured=false,engine=null}={}) {
  const result=Object.fromEntries(Object.entries(environment).filter(([key])=>!['LYCHEEDEV_DATA_PHASE_PROFILE','LYCHEEDEV_DATA_CLI_PROFILE'].includes(key.toUpperCase())));
  if(enabled&&measured&&engine==='rust'){result.LYCHEEDEV_DATA_PHASE_PROFILE='1';result.LYCHEEDEV_DATA_CLI_PROFILE='1';}
  return result;
}
export function generationPhaseProfiles(stderr,{phase,successful=false}={}) {
  assert(Buffer.isBuffer(stderr)&&stderr.length>0&&stderr.length<=32768,'bounded complete dual-profile stderr');
  assert(['fresh','prepare','first','subsequent'].includes(phase),'known measured phase');
  const text=new TextDecoder('utf-8',{fatal:true,ignoreBOM:true}).decode(stderr);
  const lines=text.replace(/\r?\n$/,'').split(/\r?\n/);assert.equal(lines.length,2,'exactly Data then CLI profile; unknown stderr rejected');
  const data=profiles.dataPhaseProfile(Buffer.from(lines[0]),{successful});
  assert.equal(data.schema,'lycheedev.data-query-phases.v5','new product must emit actual v5 SQL observations');
  if(successful){
    const counts=new Map(data.sql.map(row=>[row.phase,row.calls]));
    const required=phase==='prepare'?['generation_prepare_inclusive','generation_import_encode_inclusive','generation_object_commit','generation_manifest_publish']:['request_compile_admit','execute_resolved_inclusive','source_prepare_inclusive','source_verify_inclusive','result_projection','capture_serialize','capture_publish_metadata_inclusive','generation_open_inclusive','generation_read_admission','generation_manifest_authenticate'];
    for(const name of required)assert(counts.get(name)>0,`${name}: required successful ${phase} scope`);
    if(phase!=='prepare')for(const name of ['generation_object_open_authenticate_inclusive','generation_directory_validate_inclusive','generation_cursor_batch_decode','generation_cursor_advance_inclusive']){if(phase==='fresh')assert.equal(counts.get(name),0,`${name}: fresh WDC must not claim generation hit`);else assert(counts.get(name)>0,`${name}: generation hit scope missing`);}
  }
  const cli=profiles.dataCliProfile(Buffer.from(lines[1]));
  assert.equal(cli.command,phase==='prepare'?'data cache prepare':'data sql');
  if(successful)assert.equal(cli.renderSucceeded,true,'successful complete render required');
  return {data,cli,rawStderrBase64:stderr.toString('base64'),qualification:'Overlapping nested wall clocks are diagnostic only. Main entry excludes loader/OS work and own diagnostic emission; external-clock remainder is unattributed, not pure startup. No phase is subtracted.'};
}
export function verifyGenerationPhaseEvidence(sample,phase) {
  const value=sample.phaseDiagnostics;assert(value&&typeof value.rawStderrBase64==='string');
  const raw=Buffer.from(value.rawStderrBase64,'base64');assert.equal(raw.toString('base64'),value.rawStderrBase64,'canonical complete retained stderr');
  assert.equal(createHash('sha256').update(raw).digest('hex'),sample.stderrSHA256,'diagnostics bind original stderr bytes');
  assert.deepEqual(generationPhaseProfiles(raw,{phase,successful:true}),value,'complete independently parsed diagnostic observations');
  return true;
}
