// Own the complete test orchestrator, including descendants that outlive a
// failing Node entrypoint. Product command deadlines remain unchanged.
import {writeFileSync} from 'node:fs';
import {join,resolve} from 'node:path';
import {beginScratch} from '../../../tools/rust-disk.mjs';
import {ownedUserEnvironment} from '../../../tools/rust-build.mjs';
import {ownedSpawnSync} from '../../../tools/rust-owned-process.mjs';

// The complete group and its descendants share the queue's 600-second owner.
// Nested command budgets do not extend this deadline.
export const ORACLE_TIMEOUT_MS=600000;

export function runControlledOracle(root=process.cwd(),execute=ownedSpawnSync){
  root=resolve(root);
  const script=join(root,'tests/rust-source-engine/fixed-emmy-oracle/run.mjs');
  const job=beginScratch(root,'ci-emmy-oracle-orchestrator',{
    command:[process.execPath,script],inputs:{timeoutMs:ORACLE_TIMEOUT_MS}});
  const reportPath=join(job.evidence,'orchestrator-report.json');
  const report={schema:'lycheedev.emmy-oracle-orchestrator.v1',state:'interrupted',
    timeoutMs:ORACLE_TIMEOUT_MS,handlesReaped:false,exitCode:null};
  let logs=[];
  try{
    const result=execute(process.execPath,[script],{cwd:root,env:ownedUserEnvironment(job.directory),
      evidence:job.evidence,timeout:ORACLE_TIMEOUT_MS,orphanMs:30000});
    logs=result.logs||[];
    report.handlesReaped=result.handlesReaped===true;
    report.exitCode=result.status;
    report.error=result.error?.message||null;
    report.state=!report.handlesReaped?'interrupted':
      result.status===0&&!result.error?'passed':'failed';
  }catch(error){
    report.error=error.message;
  }finally{
    writeFileSync(reportPath,JSON.stringify(report,null,2)+'\n');
    job.seal({state:report.state==='passed'?'succeeded':report.state,
      handlesReaped:report.handlesReaped,logs:[reportPath,...logs]});
    // Keep failed orchestration scratch registered for inspection even when
    // the exact process tree was successfully retired.
    if(report.state==='passed')job.cleanup();
  }
  return {...report,receipt:job.receipt};
}

if(process.argv[1]&&resolve(process.argv[1])===resolve(import.meta.filename)){
  const report=runControlledOracle();
  console.log(JSON.stringify(report,null,2));
  if(report.state!=='passed')process.exitCode=1;
}
