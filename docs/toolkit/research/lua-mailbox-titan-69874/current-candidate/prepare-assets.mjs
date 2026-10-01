import fs from 'node:fs/promises';
const dir='docs/toolkit/research/lua-mailbox-titan-69874';
const temp='.tmp/mailbox-live/titan-current-20261001';
const load=async p=>JSON.parse((await fs.readFile(p,'utf8')).replace(/^\uFEFF/,''));
const p=await load('docs/toolkit/research/lua-mailbox/root.profile.json');
p.id='titan-lua-mailbox-root-v1-69874';p.buildKey='titan@3.80.2.69874';
p.executableSha256='f07b6fc909d0b405dd44a7d3e35efa21e749cc2ca321e9472dcf0b36c77c88c9';p.module.name='WowClassic.exe';
for(const f of Object.values(p.fields)){f.anchoredBy='titan-lua-state-root-rip-v1-69874';f.root.rva='0x6daca78';}
for(const b of p.bindings)b.recipe='titan-lua-state-root-rip-v1-69874';
for(const i of p.invariants)i.reason='Titan 69874 current-process candidate layout: standard Go public-Mailbox traversal and exact HEAD/BODY protocol acceptance succeeded. Independent root query and complete reader must verify these fields. Revalidate on every build; no unbound layout or future ABI claim.';
p.evidence=[{source:'Unique full runtime .text recipe plus current-process public-Mailbox protocol acceptance. Root-only profile; complete reader is a separate bounded probe.',semanticVerified:false,restartVerified:false}];
await fs.writeFile(dir+'/root.profile.json',JSON.stringify(p,null,2)+'\n',{flag:'wx'});
let reader=await fs.readFile('docs/toolkit/research/lua-mailbox/reader.mjs','utf8');
reader=reader.replaceAll('retail-lua-mailbox-v1-69933','titan-lua-mailbox-v1-69874').replaceAll('retail-lua-state-root-rip-v1','titan-lua-state-root-rip-v1-69874').replaceAll('retail@12.1.0.69933','titan@3.80.2.69874').replaceAll('d41f11de411f6fdb280a7c1c6380ba3d1b17614f03e5aca8fabce2cc715cd7dd',p.executableSha256).replaceAll('0x79c0c18','0x6daca78').replaceAll('=== "wow.exe"','=== "wowclassic.exe"');
await fs.writeFile(dir+'/reader.mjs',reader,{flag:'wx'});
await fs.copyFile(dir+'/reader.mjs',temp+'/workspace/scripts/mailbox-reader.mjs');
await fs.copyFile(temp+'/text-dump/manifest.json',dir+'/runtime-text.manifest.json');
await fs.writeFile(dir+'/binding.json',JSON.stringify({schema:'lycheedev.mailbox.research-binding.v1',bundleID:'titan-lua-mailbox-v1-69874',recipeID:'titan-lua-state-root-rip-v1-69874',productionRecipeID:'retail-lua-state-root-rip-v1',buildKey:p.buildKey,executableSHA256:p.executableSha256,module:'WowClassic.exe',rootRVA:'0x6daca78',anchorRVA:'0x639be4',release:'3.1.0',process:{pid:43756,creation:'134353096910506547'},scope:'current-process read; no semantics/restart/future ABI promotion',readerProbeAuthorization:false},null,2)+'\n',{flag:'wx'});
