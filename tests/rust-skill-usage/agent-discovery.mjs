// Pure checks for actual metadata calls. Never invokes a binary on import.
import assert from 'node:assert/strict';
export function assertDiscoveryNoHome(observations, ids, paths) {
 assert.deepEqual(observations.map(row => row.id), ids);
 for (const row of observations) { assert.deepEqual(row.paths, paths); assert.deepEqual(row.before, paths.map(() => false)); assert.deepEqual(row.after, paths.map(() => false)); }
}
export function assertCommandSchema(value, command) {
 assert.equal(value.schema, 'lycheedev.cli-command.v1'); assert.equal(value.command, command);
 assert(value.input && Array.isArray(value.input.arguments) && Array.isArray(value.input.flags));
 if (command === 'data agent serve') assert.equal(value.protocol.protocol, 'lycheedev.data-agent.v1');
}
export function assertSearch(value, query, expected) {
 assert.equal(value.schema, 'lycheedev.cli-search.v1'); assert.equal(value.query, query); assert.equal(value.limit, 5);
 assert(Array.isArray(value.matches) && value.matches.length <= 5);
 if (expected) assert(value.matches.some(row => row.command === expected)); else assert.deepEqual(value.matches, []);
}
export function assertHelp(value, command = '') {
 assert.equal(value.schema, 'lycheedev.cli-help.v1'); assert.equal(value.command, command);
 if (command) { assert.equal(value.kind, 'command'); assert.deepEqual(value.commands.map(row => row.path), [command]); }
 else { assert.equal(value.kind, 'group'); assert(value.children.length <= 32); assert.equal(value.limits.children, 32); }
}
