// This is a bounded custody companion, not a generic child-process executor.
// All source work stays inside Python. EOF/timeout preserves interrupted work.
import {writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {beginScratch} from '../../tools/rust-disk.mjs';
const [root, requestJSON] = process.argv.slice(2);
let job, finished = false, buffer = Buffer.alloc(0);
const reply = value => process.stdout.write(JSON.stringify(value) + '\n');
function finish(report, authenticated) {
  if (finished) return;
  finished = true;
  clearTimeout(timer);
  process.stdin.pause();
  try {
    if (!job) throw new Error('emmy.scratch_not_admitted');
    const log = join(job.evidence, 'preparation.json');
    writeFileSync(log, JSON.stringify({schema:'lycheedev.emmy-preparation.v1', ...report}, null, 2) + '\n', {flag:'wx'});
    const state = authenticated ? report.state : 'interrupted';
    job.seal({state, handlesReaped: authenticated && state !== 'interrupted', logs:[log]});
    if (authenticated && state !== 'interrupted') job.cleanup();
    reply({state, receipt: job.receipt});
  } catch (error) {
    reply({error:String(error.message).slice(0, 4096)});
    process.exitCode = 1;
  }
  process.stdin.destroy();
}
const timer = setTimeout(() => finish({state:'interrupted', error:'emmy.scratch_deadline'}, false), 360000);
try {
  if (!requestJSON || Buffer.byteLength(requestJSON) > 32768) throw new Error('emmy.scratch_request_budget');
  const request = JSON.parse(requestJSON);
  if (!Array.isArray(request.command) || request.command.length > 32 || !request.command.every(x=>typeof x==='string'&&x.length<=4096)
      || !request.inputs || typeof request.inputs!=='object' || Object.keys(request.inputs).length>64
      || Object.values(request.inputs).some(x=>typeof x!=='string'||!/^[a-f0-9]{64}$/.test(x))) throw new Error('emmy.scratch_request');
  for (const key of ['scratchReserveBytes','requiredBytes']) if (!Number.isSafeInteger(request[key]) || request[key]<0 || request[key]>160*1024*1024) throw new Error('emmy.scratch_reservation');
  job = beginScratch(root, 'emmy-source-preparation', {command:request.command, inputs:request.inputs,
    admissionOptions:{scratchReserveBytes:request.scratchReserveBytes, requiredBytes:request.requiredBytes}});
  reply({directory:job.directory, evidence:job.evidence, receipt:job.receipt});
  process.stdin.on('data', chunk => {
    if (finished) return;
    if (buffer.length + chunk.length > 65536) return finish({state:'interrupted',error:'emmy.scratch_terminal_budget'}, false);
    buffer = Buffer.concat([buffer, chunk]);
    const newline = buffer.indexOf(10);
    if (newline<0) return;
    if (newline!==buffer.length-1) return finish({state:'interrupted',error:'emmy.scratch_terminal_trailing'}, false);
    try {
      const report = JSON.parse(buffer.subarray(0,newline));
      if (!['succeeded','failed','interrupted'].includes(report.state) || ![null,'string'].includes(report.error===null?null:typeof report.error)
          || (report.treeSHA256!==null&&!/^[a-f0-9]{64}$/.test(report.treeSHA256))
          || (report.state==='succeeded'&&report.error!==null)) throw new Error('emmy.scratch_terminal');
      finish(report, true);
    } catch (error) { finish({state:'interrupted',error:String(error.message).slice(0,4096)}, false); }
  });
  process.stdin.on('end', () => finish({state:'interrupted',error:'emmy.scratch_parent_eof'}, false));
  process.stdin.on('error', error => finish({state:'interrupted',error:String(error.message).slice(0,4096)}, false));
} catch (error) {
  clearTimeout(timer);
  reply({error:String(error.message).slice(0,4096)});
  process.exitCode = 1;
}
