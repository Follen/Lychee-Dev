// Frozen old/new CSV contracts remain historical exports, never current admission.
export * from './historical-data-csv-diagnostics.mjs';
import {resolve} from 'node:path';
if(process.argv[1]&&resolve(process.argv[1])===resolve(import.meta.filename)){
 const {main}=await import('./agent-paired-owned.mjs');
 await main('csv');
}
