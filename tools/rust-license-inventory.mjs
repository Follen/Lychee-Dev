import {readFileSync,readdirSync} from 'node:fs';
import {dirname,join,resolve,sep} from 'node:path';
import {createHash} from 'node:crypto';
import {emmySourcePath,inspectEmmy} from './rust-emmy-identity.mjs';

const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
export function licensedDependencies(root,metadata,supplements){
  root=resolve(root);
  const emmyRoot=join(root,emmySourcePath);
  const emmyPackage=pkg=>resolve(pkg.manifest_path).startsWith(join(emmyRoot,'crates')+sep);
  const reachable=new Set(metadata.resolve.nodes.map(node=>node.id));
  for(const pkg of metadata.packages)if(reachable.has(pkg.id)&&!pkg.source&&pkg.name.startsWith('emmylua_')&&!emmyPackage(pkg))
    throw new Error(`dependencies.emmy_source_outside_custody: ${pkg.name}`);
  const packages=metadata.packages.filter(pkg=>reachable.has(pkg.id)&&(pkg.source||emmyPackage(pkg)))
    .sort((a,b)=>a.name.localeCompare(b.name)||a.version.localeCompare(b.version));
  const emmy=packages.some(emmyPackage)?inspectEmmy(root):null;
  if(emmy)for(const pkg of packages.filter(emmyPackage)){
    const path=resolve(pkg.manifest_path).slice(emmyRoot.length+1).replaceAll('\\','/');
    if(pkg.license!=='MIT'||pkg.repository!==emmy.base.repository)throw new Error(`dependencies.emmy_license_identity: ${pkg.name}`);
    if(!emmy.files.some(file=>file.path===path))throw new Error(`dependencies.emmy_manifest_missing: ${pkg.name}`);
  }
  const records=[];
  for(const pkg of packages){
    if(!pkg.license)throw new Error(`dependencies.license_missing: ${pkg.name}`);
    const directory=dirname(pkg.manifest_path);
    const files=readdirSync(directory).filter(name=>/^(license|copying|notice|copyright)([-_.].*)?$/i.test(name));
    const texts=[...new Set([...files,...(pkg.license_file?[pkg.license_file]:[])])].map(name=>{
      const bytes=readFileSync(join(directory,name));return{name,sha256:hash(bytes),text:bytes.toString('utf8')};
    });
    if(!texts.length&&emmyPackage(pkg)){
      if(pkg.license!=='MIT'||pkg.repository!=='https://github.com/CppCXY/emmylua-analyzer-rust')
        throw new Error(`dependencies.emmy_license_identity: ${pkg.name}`);
      // New reconstruction custody verifies these exact original MIT bytes.
      const path=join(emmyRoot,'LICENSE');
      const bytes=readFileSync(path);
      texts.push({name:'EmmyLua/LICENSE',sha256:hash(bytes),text:bytes.toString('utf8'),
        source:`https://github.com/CppCXY/emmylua-analyzer-rust/blob/${emmy.base.commit}/LICENSE`});
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
    if(emmyPackage(pkg)&&!texts.some(text=>text.sha256===emmy.base.license.sha256))throw new Error(`dependencies.emmy_license_bytes: ${pkg.name}`);
    records.push({name:pkg.name,version:pkg.version,license:pkg.license,source:pkg.source,
      repository:pkg.repository,licenseFiles:texts,...(emmyPackage(pkg)?{
        correspondingSource:{schema:emmy.schema,historicalPrivatePatchRecovered:false,baseCommit:emmy.base.commit,
          baseArchiveSHA256:emmy.base.archiveSHA256,identitySHA256:emmy.identitySHA256,recipeSHA256:emmy.recipeSHA256,
          patchedTreeSHA256:emmy.patchedTreeSHA256,path:resolve(pkg.manifest_path).slice(emmyRoot.length+1).replaceAll('\\','/')}
      }:{})});
  }
  if(emmy){const after=inspectEmmy(root);if(after.recipeSHA256!==emmy.recipeSHA256||after.patchedTreeSHA256!==emmy.patchedTreeSHA256)throw new Error('dependencies.emmy_changed_during_inventory');}
  return records;
}
