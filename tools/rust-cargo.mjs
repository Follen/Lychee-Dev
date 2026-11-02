// All compilation commands share the project admission, owner and channel.
import {runCargo} from './rust-build.mjs';

const argv=process.argv.slice(2),separator=argv.indexOf('--');
if(separator<0)throw new Error('usage: node tools/rust-cargo.mjs [--owner name] [--one-shot] -- <build|test|check|clippy|fetch|metadata> <cargo arguments>');
const options=argv.slice(0,separator);let owner='rust-cargo',oneShot=false;
for(let i=0;i<options.length;i++){
  if(options[i]==='--owner'&&options[i+1])owner=options[++i];
  else if(options[i]==='--one-shot')oneShot=true;
  else throw new Error('build.invalid_runner_option');
}
try{
  const result=runCargo(process.cwd(),argv.slice(separator+1),{owner,oneShot});
  process.stdout.write(result.stdout);process.stderr.write(result.stderr);
  console.log(JSON.stringify({buildEvidence:result.buildEvidence,target:result.target,inputHashes:result.inputHashes}));
}catch(error){console.error(JSON.stringify({error:error.message,buildEvidence:error.buildEvidence,admission:error.report}));process.exitCode=1;}
