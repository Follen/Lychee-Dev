import assert from 'node:assert/strict';
import {policy,defaults} from '../../tools/rust-disk.mjs';
// Shared admission, not the broader query/fixture module graph.
export function agentEnvironment(root,environment){assert.deepEqual(policy(root,{}),{...defaults},'unchanged owned disk policy');for(const[k,v]of Object.entries(environment)){assert(!/^LYCHEEDEV_DISK_/i.test(k),'disk overrides forbidden');if(['RUSTFLAGS','CARGO_ENCODED_RUSTFLAGS','CARGO_TARGET_DIR','RUST_MIN_STACK'].includes(k.toUpperCase()))assert(!v,'default compiler/target/stack required');assert(!/^LYCHEEDEV_.*(?:PROFILE|DIAGNOSTIC|BUDGET|LIMIT|CACHE)/i.test(k),'unrequested runtime override');}return{...environment};}
export const preparationEnvironmentPolicy='default-stack-flags-disk-v1';
export function preparationTimeout(started,now,owned){assert(Number.isFinite(started)&&Number.isFinite(now)&&now>=started);assert(Number.isSafeInteger(owned)&&owned>=1000&&owned<=600000);const remaining=Math.floor(600000-(now-started));assert(remaining>=1000,'fixture preparation shared deadline');return Math.min(owned,remaining);}
