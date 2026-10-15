import { readFile, writeFile, mkdir, lstat, readdir } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { randomBytes, createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const [action, ...args] = process.argv.slice(2);
const sha = b => createHash('sha256').update(b).digest('hex');
const binary = path.join(root, '.tmp', 'bridge-input-lab.exe');
function exec(command, values) {
  const r = spawnSync(command, values, { cwd: root, stdio: 'inherit', windowsHide: true });
  if (r.error) throw r.error;
  if (r.status !== 0) process.exit(r.status ?? 1);
}
async function install() {
  if (args.length !== 2 || args[0] !== '--installation' || !path.isAbsolute(args[1])) throw Error('install --installation <absolute client directory>');
  const client = path.resolve(args[1]);
  const flavor = await readFile(path.join(client, '.flavor.info'), 'utf8');
  const parent = path.join(client, 'Interface', 'AddOns');
  const target = path.join(parent, 'LycheeInputLab');
  const token = randomBytes(8).toString('hex');
  const files = new Map();
  for (const name of ['LycheeInputLab.toc','Protocol.lua','Lab.lua']) files.set(name, await readFile(path.join(root,'tests/bridge-input/addon',name)));
  files.set('MatrixSymbol.lua', await readFile(path.join(root,'addon/Bridge/MatrixSymbol.lua')));
  files.set('Config.lua', Buffer.from(`local _, ns = ...\nns.Config = { token = "${token}" }\n`));
  // Refuse unknown or edited directories. This lab never writes into Lychee Dev.
  let exists = false;
  try { const st = await lstat(target); if (!st.isDirectory() || st.isSymbolicLink()) throw Error('Unsafe lab target'); exists = true; }
  catch (e) { if (e.code !== 'ENOENT') throw e; }
  if (exists) {
    const previous = JSON.parse(await readFile(path.join(target,'lab-manifest.json'),'utf8'));
    if (previous.schema !== 'lycheedev.input-lab-install.v1') throw Error('Unmanaged lab directory');
    for (const name of await readdir(target)) {
      if (name === 'lab-manifest.json') continue;
      const st = await lstat(path.join(target,name));
      if (!st.isFile() || st.isSymbolicLink() || previous.files[name] !== sha(await readFile(path.join(target,name)))) throw Error(`Modified lab file: ${name}`);
    }
  } else { await mkdir(target); }
  const hashes = {};
  for (const [name, bytes] of files) { await writeFile(path.join(target,name), bytes); hashes[name] = sha(bytes); }
  const manifest = { schema:'lycheedev.input-lab-install.v1', token, files:hashes, flavor:flavor.trim(), installedAt:new Date().toISOString() };
  await writeFile(path.join(target,'lab-manifest.json'), JSON.stringify(manifest,null,2)+'\n');
  console.log(JSON.stringify({target,token,files:[...files.keys()],activation:'not_run'},null,2));
}
await mkdir(path.join(root,'.tmp'), {recursive:true});
if (action === 'install') await install();
else if (action === 'build') exec('go',['build','-tags','lycheedev_input_lab','-o',binary,'./tests/bridge-input/host']);
else if (action === 'run') exec(binary,args);
else if (action === 'test') {
  exec('go',['test','-tags','lycheedev_input_lab','-count=1','./internal/desktop','./tests/bridge-input/...']);
} else throw Error('Use build | install --installation <client> | run --pid <pid> --installation <client> --action inspect|observe|probe|reload|dismiss --out <new directory> [--mode postmessage|sendinput] | test');
