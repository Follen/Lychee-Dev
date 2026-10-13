// Exhaustive frozen inventory is separate from the currently completed handlers.
import { readFileSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { resolve, join } from 'node:path';
const root=resolve(process.cwd());
const inventory=JSON.parse(readFileSync(join(root,'crates/lycheedev-cli/src/commands.json')));
const binary=resolve(process.argv[2]||join(root,'target/debug/lycheedev.exe'));
const result=spawnSync(binary,['describe','--format','json'],{encoding:'utf8',windowsHide:true,maxBuffer:4*1024*1024});
if(result.status!==0)throw new Error(`rust-contract.describe_failed: ${result.stderr}`);
const actual=JSON.parse(result.stdout).result.commands;const byPath=new Map(actual.map(command=>[command.path,command]));
const report={schema:'lycheedev.rust-command-migration.v1',inventory:inventory.length,registered:actual.length,commands:inventory.map(command=>({path:command.path,state:byPath.has(command.path)?'registered_requires_parity':'not_implemented',contract:byPath.get(command.path)}))};
for(const command of actual){const original=inventory.find(before=>before.path===command.path);if(!original||JSON.stringify(original)!==JSON.stringify(command))throw new Error(`rust-contract.public_drift: ${command.path}`);}
const output=process.argv.slice(3).find(argument=>!argument.startsWith('--'));
if(output)writeFileSync(resolve(output),JSON.stringify(report,null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({inventory:report.inventory,registered:report.registered,missing:report.commands.filter(c=>c.state==='not_implemented').map(c=>c.path),complete:false}));
if(process.argv.includes('--require-complete')&&report.registered!==report.inventory)process.exitCode=1;
