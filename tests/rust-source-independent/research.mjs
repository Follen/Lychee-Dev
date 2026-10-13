// Independent public Go/Rust research profiles on a fresh complete immutable fixture.
// This is functional precision evidence, never the release performance matrix.
import {spawnSync} from 'node:child_process';
import {mkdirSync,readFileSync,writeFileSync} from 'node:fs';
import {resolve,join} from 'node:path';
import {createHash} from 'node:crypto';
const root=resolve(process.cwd()),out=resolve(process.argv[2]),rust=resolve(process.argv[3]),go=join(root,'.tmp/lycheedev-oracle.exe'),prepare=join(root,'target/debug/examples/prepare_broker.exe');
mkdirSync(out);const hash=path=>createHash('sha256').update(readFileSync(path)).digest('hex');
const identity={rust:{path:rust,sha256:hash(rust)},go:{path:go,sha256:hash(go)},fixture:{path:prepare,sha256:hash(prepare)}};
const setup=spawnSync(prepare,[join(out,'home')],{encoding:'utf8',windowsHide:true,timeout:60000,maxBuffer:4<<20});
writeFileSync(join(out,'fixture.stdout.json'),setup.stdout||'');writeFileSync(join(out,'fixture.stderr.txt'),setup.stderr||'');
if(setup.status!==0)throw Error('Immutable fixture preparation failed');
const fixture=JSON.parse(setup.stdout),home=fixture.home,pin=fixture.pin.id;
function run(backend,name,args){const response=spawnSync(backend==='rust'?rust:go,[...args,'--snapshot',pin,'--static-only','--home',home,'--format','json'],{encoding:'utf8',windowsHide:true,shell:false,timeout:90000,maxBuffer:16<<20});writeFileSync(join(out,`${name}-${backend}.stdout.json`),response.stdout||'');writeFileSync(join(out,`${name}-${backend}.stderr.txt`),response.stderr||'');let envelope;try{envelope=JSON.parse(response.stdout);}catch{}return{exit:response.status,error:response.error?.message,envelope};}
const cases=[
 ['refs-both',['source','refs','--symbol','First']],
 ['refs-incoming',['source','refs','--symbol','First','--direction','incoming']],
 ['refs-outgoing',['source','refs','--symbol','First','--direction','outgoing']],
 ['refs-limit',['source','refs','--symbol','First','--limit','1']],
 ['context-default',['source','context','--symbol','First']],
 ['context-depth-zero',['source','context','--symbol','First','--depth','0']],
 ['context-depth-two',['source','context','--symbol','First','--depth','2']],
 ['context-lines',['source','context','--symbol','First','--max-lines','1']],
 ['context-bytes',['source','context','--symbol','First','--max-bytes','7']],
 ['refs-direction-invalid',['source','refs','--symbol','First','--direction','wrong']],
 ['refs-selector-missing',['source','refs']],
 ['refs-selector-conflict',['source','refs','--symbol','First','--symbol-id','different']],
 ['refs-symbol-id-missing',['source','refs','--symbol-id','SYM-00000000000000000000000000000000']],
 ['refs-cursor-invalid',['source','refs','--symbol','First','--cursor','bad']],
 ['context-cursor-invalid',['source','context','--symbol','First','--cursor','bad']],
 ['context-depth-invalid',['source','context','--symbol','First','--depth','5']],
 ['context-bytes-invalid',['source','context','--symbol','First','--max-bytes','0']],
 ['context-lines-invalid',['source','context','--symbol','First','--max-lines','0']],
];
const records=[];
const canonical=value=>Array.isArray(value)?value.map(canonical):value&&typeof value==='object'?Object.fromEntries(Object.keys(value).sort().map(key=>[key,canonical(value[key])])):value;
// Only these explicit additive/derived projection differences are excluded.
// Every remaining manifest/coverage property is compared, not a tolerance.
const projection=value=>{const result=structuredClone(value);if(!result||typeof result!=='object')return result;
 if(result.environmentManifest?.identity?.generatorVersion==='environment-ts-v1')result.environmentManifest.identity.generatorVersion='environment-v1';
 delete result.semanticAnalysis;
 if(result.coverage?.reasons)result.coverage.reasons=result.coverage.reasons.filter(reason=>reason!=='context output exceeded the selected line, byte or relation budget');
 for(const row of result.loadEvidence||[])delete row.sourceBlob;
 return result;};
for(const[name,args]of cases){const original=run('go',name,args),candidate=run('rust',name,args);const before=projection(original.envelope?.result),after=projection(candidate.envelope?.result);const fields=[...new Set([...Object.keys(before||{}),...Object.keys(after||{})])];const mismatches=fields.filter(field=>JSON.stringify(canonical(before?.[field]))!==JSON.stringify(canonical(after?.[field])));const errorEqual=JSON.stringify(canonical(original.envelope?.error))===JSON.stringify(canonical(candidate.envelope?.error));const contextEqual=JSON.stringify(canonical(original.envelope?.context))===JSON.stringify(canonical(candidate.envelope?.context));records.push({name,args,goExit:original.exit,rustExit:candidate.exit,mismatches,errorEqual,contextEqual,passed:original.exit===candidate.exit&&mismatches.length===0&&errorEqual&&contextEqual});}
const finalIdentity={rust:hash(rust),go:hash(go),fixture:hash(prepare)};const stable=finalIdentity.rust===identity.rust.sha256&&finalIdentity.go===identity.go.sha256&&finalIdentity.fixture===identity.fixture.sha256;const report={schema:'lycheedev.research-public-independent.v1',state:records.every(row=>row.passed)&&stable?'passed':'failed',identity,finalIdentity,stable,home,pin,records,allowedProjectionDifferences:['environmentManifest.identity.generatorVersion derived environment-ts-v1','additive semanticAnalysis','additive loadEvidence.sourceBlob','additive explicit context budget coverage reason'],scope:'complete synthetic fixed Git source/catalog; every remaining result property and error/context compared for explicit static-only profiles; semantic/broker/flow/full-source/client/performance matrices not covered'};writeFileSync(join(out,'report.json'),JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify({state:report.state,records,report:join(out,'report.json')}));if(report.state!=='passed')process.exitCode=1;
