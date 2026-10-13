import {readFileSync,readdirSync} from 'node:fs';
import {dirname,join,resolve,sep} from 'node:path';
import {createHash} from 'node:crypto';
import {inspectContext} from '../tests/rust-source-engine/context-identity.mjs';

const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
export function licensedDependencies(root,metadata,supplements){
  root=resolve(root);
  const privateRoot=join(root,'.tmp/rust-source-binding-context-01/upstream');
  const privatePackage=pkg=>resolve(pkg.manifest_path).startsWith(join(privateRoot,'crates')+sep);
  const reachable=new Set(metadata.resolve.nodes.map(node=>node.id));
  const packages=metadata.packages.filter(pkg=>reachable.has(pkg.id)&&(pkg.source||privatePackage(pkg)))
    .sort((a,b)=>a.name.localeCompare(b.name)||a.version.localeCompare(b.version));
  const emmy=packages.some(privatePackage)?inspectContext(root):null;
  const records=[];
  for(const pkg of packages){
    if(!pkg.license)throw new Error(`dependencies.license_missing: ${pkg.name}`);
    const directory=dirname(pkg.manifest_path);
    const files=readdirSync(directory).filter(name=>/^(license|copying|notice|copyright)([-_.].*)?$/i.test(name));
    const texts=[...new Set([...files,...(pkg.license_file?[pkg.license_file]:[])])].map(name=>{
      const bytes=readFileSync(join(directory,name));return{name,sha256:hash(bytes),text:bytes.toString('utf8')};
    });
    if(!texts.length&&privatePackage(pkg)){
      if(pkg.license!=='MIT'||pkg.repository!=='https://github.com/CppCXY/emmylua-analyzer-rust')
        throw new Error(`dependencies.private_license_identity: ${pkg.name}`);
      // The complete immutable upstream inventory authenticates the original
      // copyright and grant. The patched crate retains that same MIT license.
      const path=join(root,'.tmp/rust-source-engine-research-01/upstream/LICENSE');
      const bytes=readFileSync(path);
      texts.push({name:'EmmyLua/LICENSE',sha256:hash(bytes),text:bytes.toString('utf8'),
        source:`https://github.com/CppCXY/emmylua-analyzer-rust/blob/${emmy.baseIdentity.commit}/LICENSE`});
    }
    if(!texts.length){
      const supplement=supplements.find(record=>record.name===pkg.name&&record.version===pkg.version);
      if(!supplement)throw new Error(`dependencies.license_text_missing: ${pkg.name}`);
      const vcs=JSON.parse(readFileSync(join(directory,'.cargo_vcs_info.json')));
      if(vcs.git.sha1!==supplement.commit||pkg.repository!==supplement.repository)
        throw new Error(`dependencies.license_source_mismatch: ${pkg.name}`);
      const bytes=readFileSync(join(root,'release/licenses',supplement.path)),digest=hash(bytes);
      if(digest!==supplement.sha256)throw new Error(`dependencies.license_digest_mismatch: ${pkg.name}`);
      texts.push({name:supplement.path,sha256:digest,text:bytes.toString('utf8'),source:supplement.source,
        ...(supplement.sourceCommit?{sourceCommit:supplement.sourceCommit,sourceScope:supplement.sourceScope,
          exactCrateRevisionLicenseVerified:supplement.exactCrateRevisionLicenseVerified,reason:supplement.reason}:{})});
    }
    records.push({name:pkg.name,version:pkg.version,license:pkg.license,source:pkg.source,
      repository:pkg.repository,licenseFiles:texts,...(privatePackage(pkg)?{
        correspondingSource:{baseCommit:emmy.baseIdentity.commit,baseManifestSHA256:emmy.baseIdentity.manifestSHA256,
          patchSHA256:emmy.patchSHA256,path:resolve(pkg.manifest_path).slice(privateRoot.length+1).replaceAll('\\','/')}
      }:{})});
  }
  return records;
}
