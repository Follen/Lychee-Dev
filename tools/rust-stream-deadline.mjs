// Reconstructed Agent stdout-backpressure probe. A pause alone is not a pass.
// A small real export which fits the pipe is reported as not triggered.
import {runOwnedAgentStream} from './rust-agent-stream-probe.mjs';

runOwnedAgentStream('backpressure-deadline', process.argv.slice(2));
