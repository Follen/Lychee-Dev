// Read-only independent replay, itself in the same official owner Job tree.
import assert from 'node:assert/strict';
import {join} from 'node:path';
import {performance} from 'node:perf_hooks';
import {hashFile,sourceDigest,verifyArtifact} from '../../tools/rust-build.mjs';
import {verifyGoPackage} from './inputs.mjs';
import {parse} from './agent-json.mjs';
import {bounded,digest} from './agent-fixture-inputs.mjs';
import {authenticate} from './agent-custody.mjs';
import {replay} from './agent-report.mjs';
assert.equal(process.platform,'win32');assert.equal(process.argv.length,3);const request=parse(bounded(process.argv[2],4<<20));assert.equal(request.schema,'lycheedev.paired-agent-replay-request.recovered.v1');const started=performance.now();assert(Number.isSafeInteger(request.remainingMs)&&request.remainingMs>0&&request.remainingMs<=600000);
const raw=bounded(request.configuration.path,2<<20);assert.equal(digest(raw),request.configuration.sha256);const c=parse(raw),reportBytes=bounded(request.report.path,16<<20);assert.equal(digest(reportBytes),request.report.sha256);const report=parse(reportBytes);
function stable(){assert.equal(hashFile(c.artifact,65536),c.artifactSHA256);assert.equal(hashFile(c.importerReceipt,4<<20),c.importerReceiptSHA256);for(const f of request.logs)assert.equal(hashFile(f.path,16<<20),f.sha256);for(const[p,h]of Object.entries(c.inputs))assert.equal(hashFile(join(c.runnerRoot,p)),h);assert.equal(sourceDigest(c.root),c.rust.inputHashes.sourceTree);assert.equal(verifyArtifact(c.root,c.artifact).binarySHA256,c.rust.binarySHA256);assert.equal(verifyGoPackage(c.goPackage,c.luaLSArchive).packageSHA256,c.go.packageSHA256);}
stable();const owner={sealedLogs:request.logs.map(l=>({...l,original:l.path}))},receipt=join(c.evidence,'receipt.json'),sample=request.controller;
authenticate(sample,{owner,receipt,root:c.root,binary:c.node,args:[join(c.runnerRoot,'tests/rust-performance-paired/agent-paired-run.mjs'),request.configuration.path],inputs:c.inputs,ids:new Set(),outer:{pid:c.launcherPid,timing:sample.proof.timing},measured:false,maximumTimeout:600000});
const result=await replay(c,report,sample.proof,request.logs);stable();assert(performance.now()-started<request.remainingMs,'replay shared remaining deadline exhausted');process.stdout.write(JSON.stringify({...result,configurationSHA256:request.configuration.sha256,reportSHA256:request.report.sha256,controllerRunId:sample.proof.runId})+'\n');
