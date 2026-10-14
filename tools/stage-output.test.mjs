import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, existsSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { prepareStageOutput, stageMarker } from './stage-output.mjs';
test('stage replaces only recognized output and preserves source and unknown files', t => {
 const root=mkdtempSync(join(tmpdir(),'stage-boundary-'));t.after(()=>rmSync(root,{recursive:true,force:true}));
 const repo=join(root,'repo');mkdirSync(join(repo,'addon'),{recursive:true});
 const source=join(repo,'addon','keep.lua');writeFileSync(source,'keep');
 const out=join(repo,'.tmp','stage');assert.equal(prepareStageOutput(out,repo),out);
 mkdirSync(out,{recursive:true});writeFileSync(join(out,'unknown'),'keep');
 assert.throws(()=>prepareStageOutput(out,repo),/unknown_output/);assert.ok(existsSync(join(out,'unknown')));
 assert.throws(()=>prepareStageOutput(join(repo,'addon'),repo),/source_overlap/);assert.ok(existsSync(source));
 const old=join(root,'old');mkdirSync(old);writeFileSync(join(old,stageMarker),JSON.stringify({schema:'lycheedev.live-stage.v1'}));mkdirSync(join(old,'payload'));
 assert.equal(prepareStageOutput(old,repo),old);assert.equal(existsSync(old),false);
});
