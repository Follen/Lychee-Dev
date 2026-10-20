// Current Agent-native consumer. Historical SQL reports retain their explicit historical entry.
import {main} from '../tests/rust-performance-paired/agent-paired-owned.mjs';
await main('generation');
