// Reconstructed Agent consumer-closure probe, not a historical stream receipt.
// The supplied executable, home, snapshot and installation remain authoritative.
import {runOwnedAgentStream} from './rust-agent-stream-probe.mjs';

runOwnedAgentStream('closed-consumer', process.argv.slice(2));
