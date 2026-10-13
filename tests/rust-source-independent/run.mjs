// Independent public CLI checks. Fixed source observations are not real-client acceptance.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { performance } from 'node:perf_hooks';
import { createHash } from 'node:crypto';
const root = resolve(process.cwd());
const [homeArg, pin, outputArg,...extra] = process.argv.slice(2);
if (!homeArg || !pin || !outputArg) throw new Error('usage: node tests/rust-source-independent/run.mjs <home> <fixed-pin> <new-output-directory>');
const home = resolve(homeArg), output = resolve(outputArg); mkdirSync(output);
const options={};for(let i=0;i<extra.length;i+=2){if(!['--rust-bin','--profile','--samples'].includes(extra[i])||!extra[i+1])throw new Error('invalid independent runner option');options[extra[i]]=extra[i+1];}
const rust = resolve(options['--rust-bin']||process.env.LYCHEEDEV_RUST_TEST_BIN||join(root, 'target/debug/lycheedev.exe')), oracle = join(root, '.tmp/lycheedev-oracle.exe');
const profile=options['--profile']||'debug',sampleCount=Number(options['--samples']||'1');if(!['debug','release'].includes(profile)||!Number.isSafeInteger(sampleCount)||sampleCount<1||sampleCount>100)throw new Error('invalid profile/sample count');
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
const artifacts={rust:{binary:rust,sha256:hash(readFileSync(rust)),profile},oracle:{binary:oracle,sha256:hash(readFileSync(oracle)),profile:'go default optimized build'}};
const report = { schema: 'lycheedev.rust-source-independent.v2', source: JSON.parse(readFileSync(join(root, '.tmp/oracle-identity.json'))),artifacts, pin,cacheCondition:'retained workspace; derived cold state is not controlled; first-process measurement is not labeled cold cache', checks: [],qualityState:'not_run',performance50State:'not_run', remaining: ['local/alias/member synthetic corpus', 'Unicode/CRLF', 'cross-file semantic navigation', 'multi-client API environments', 'LuaLS public CLI definitions/references', 'source diff', 'source validate'] };
function run(binary, name, args) {
  const start = performance.now(); const result = spawnSync(binary, [...args, '--home', home, '--format', 'json'], { cwd: root, windowsHide: true, encoding: 'utf8', timeout: 300000, maxBuffer: 16 * 1024 * 1024 });
  writeFileSync(join(output, `${name}.stdout.json`), result.stdout || ''); writeFileSync(join(output, `${name}.stderr.txt`), result.stderr || '');
  let envelope; try { envelope = JSON.parse(result.stdout); } catch {}
  const artifact=binary===rust?artifacts.rust:artifacts.oracle;
  return { status: result.status, signal: result.signal, error: result.error?.message, bytes: Buffer.byteLength(result.stdout || ''), milliseconds: performance.now() - start, envelope,artifact,pin,args,outputSHA256:hash(Buffer.from(result.stdout||'')) };
}
function check(name, work) { try { report.checks.push({ name, state: 'passed', ...work() }); } catch (error) { report.checks.push({ name, state: 'failed', reason: error.message }); } writeFileSync(join(output, 'report.json'), JSON.stringify(report, null, 2)); }
check('original excerpt parity', () => {
  const args = ['source', 'inspect', '--snapshot', pin, '--path', 'Interface/AddOns/Blizzard_UIParent/UIParent.lua', '--line', '1', '--count', '20'];
  const before = run(oracle, 'inspect-go', args), after = run(rust, 'inspect-rust', args); assert.equal(before.status, 0); assert.equal(after.status, 0);
  for (const field of ['repository', 'commit', 'path', 'firstLine', 'lastLine', 'totalLines', 'text', 'blob']) assert.deepEqual(after.envelope.result[field], before.envelope.result[field], field);
  return { old: { bytes: before.bytes, milliseconds: before.milliseconds }, rust: { bytes: after.bytes, milliseconds: after.milliseconds } };
});
check('C_Timer.After exact API query first process and retained workspace samples', () => {
  const args = ['source', 'query', 'C_Timer.After', '--snapshot', pin, '--limit', '3'];
  const before = run(oracle, 'query-go', args), cold = run(rust, 'query-rust-cold', args), hot = run(rust, 'query-rust-hot', args);
  assert.equal(before.status, 0); assert.equal(cold.status, 0, JSON.stringify(cold.envelope?.error)); assert.equal(hot.status, 0, JSON.stringify(hot.envelope?.error));
  const result = cold.envelope.result; assert.equal(result.resolvedCommit, '31c7f7b9cc79e56c986b365c06a6afbcf3c9177b');
  assert.equal(result.snapshotId,pin);assert.equal(result.results[0].name,'C_Timer.After');assert.equal(result.results[0].path,'Interface/AddOns/Blizzard_APIDocumentationGenerated/UITimerDocumentation.lua');
  assert.equal(result.results[0].path,before.envelope.result.results[0].path,'old exact API declaration retained');
  const identityFields=['symbolId','kind','name','path','matchedBy','role','confidence','excerpt','contentHash','line','score','scoreParts'];
  const project=value=>Object.fromEntries(identityFields.map(field=>[field,value[field]]));
  assert.deepEqual(result.results.map(project),before.envelope.result.results.map(project),'all returned old result fields retain exact content');
  const samples=[];
  for(let i=0;i<sampleCount;i++){let goSample,rustSample;if(i%2===0){goSample=run(oracle,`sample-${i}-go`,args);rustSample=run(rust,`sample-${i}-rust`,args);}else{rustSample=run(rust,`sample-${i}-rust`,args);goSample=run(oracle,`sample-${i}-go`,args);}assert.equal(goSample.status,0);assert.equal(rustSample.status,0);assert.deepEqual(rustSample.envelope.result.results.map(project),goSample.envelope.result.results.map(project));const summarize=sample=>({milliseconds:sample.milliseconds,bytes:sample.bytes,artifact:sample.artifact,pin,args,outputSHA256:sample.outputSHA256,matches:sample.envelope.result.results.length,coverage:sample.envelope.result.coverage});samples.push({order:i%2===0?'go-rust':'rust-go',go:summarize(goSample),rust:summarize(rustSample)});}
  const stats=key=>{const values=samples.map(s=>s[key].milliseconds).sort((a,b)=>a-b);return{medianMs:values.length%2?values[(values.length-1)/2]:(values[values.length/2-1]+values[values.length/2])/2,p95Ms:values[Math.min(values.length-1,Math.ceil(values.length*.95)-1)]};};
  const goStats=stats('go'),rustStats=stats('rust');report.performance50State=profile!=='release'||sampleCount<20?'insufficient_measurement':rustStats.medianMs<=goStats.medianMs*.5?'passed_for_this_hot_exact_case':'failed_for_this_hot_exact_case';
  return { oracleFirstProcess: { bytes: before.bytes, milliseconds: before.milliseconds }, rustFirstProcess: { bytes: cold.bytes, milliseconds: cold.milliseconds }, rustRetainedProcess: { bytes: hot.bytes, milliseconds: hot.milliseconds },performance:{profile,samples,go:goStats,rust:rustStats,ratio:rustStats.medianMs/goStats.medianMs,target:.5,state:report.performance50State}, oldKeys: Object.keys(before.envelope.result), newKeys: Object.keys(result) };
});
report.qualityState=report.checks.every(check=>check.state==='passed')?'passed_for_selected_cases':'failed';report.state = report.qualityState==='passed_for_selected_cases' ? 'partial_passed' : 'failed';
writeFileSync(join(output, 'report.json'), JSON.stringify(report, null, 2)); console.log(JSON.stringify(report)); process.exitCode = report.state === 'failed' ? 1 : 0;
