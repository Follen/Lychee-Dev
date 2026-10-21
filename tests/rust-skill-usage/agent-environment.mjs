// One deterministic environment contract; hashes only, never archive inherited
// secret values. The owned parent and controller independently derive it.
import {join,delimiter} from 'node:path';
import {createHash} from 'node:crypto';
export function agentEnvironment(config,parent=process.env){
 const env=Object.fromEntries(Object.entries(parent).filter(([name])=>!/^(LYCHEEDEV_|NPM_CONFIG_)/i.test(name)&&!['PATH','HOME','USERPROFILE','CODEX_HOME','APPDATA','LOCALAPPDATA','TEMP','TMP','NODE_OPTIONS','NODE_PATH','PSMODULEPATH','PSEXECUTIONPOLICYPREFERENCE'].includes(name.toUpperCase())));
 Object.assign(env,{PATH:config.prefix+delimiter+(parent.PATH??parent.Path??''),HOME:join(config.scratch,'user'),USERPROFILE:join(config.scratch,'user'),CODEX_HOME:join(config.scratch,'user/.codex'),APPDATA:join(config.scratch,'user/AppData/Roaming'),LOCALAPPDATA:join(config.scratch,'user/AppData/Local'),TEMP:join(config.scratch,'temp'),TMP:join(config.scratch,'temp')});return env;
}
export const environmentHash=env=>createHash('sha256').update(JSON.stringify(Object.entries(env).sort(([a],[b])=>a.localeCompare(b)))).digest('hex');
