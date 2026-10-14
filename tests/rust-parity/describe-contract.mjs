// Pure comparison of an executed `describe` response with the fixed Go fixture.
// The caller owns binary custody, process execution, and persisted evidence.
import { GO_COMMIT, diffByKey, validateCommands } from './oracle.mjs';

const fields = ['summary', 'mutates', 'arguments', 'flags'];
// These two exact engine-label substitutions are authorized by the migration.
// Keep every raw difference visible; never normalize arbitrary LuaLS/Emmy text.
export const APPROVED_ENGINE_SUMMARIES = Object.freeze([
  Object.freeze({path: 'doctor',
    expected: 'Aggregate read-only workspace, data, LuaLS and source checks; when a live target or CON is selected, include the same checks as live doctor; exit 3 means an unhealthy check',
    actual: 'Aggregate read-only workspace, data, embedded Emmy worker smoke and source checks; when a live target or CON is selected, include the same checks as live doctor; exit 3 means an unhealthy check'}),
  Object.freeze({path: 'source batch',
    expected: 'Run 1-32 query/refs/context requests from a JSON file against one fixed snapshot, sharing preparation and a request-scoped LuaLS session; each item retains its own cursor and coverage; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins',
    actual: 'Run 1-32 query/refs/context requests from a JSON file against one fixed snapshot, sharing preparation and a request-scoped native Emmy session; each item retains its own cursor and coverage; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins'}),
]);

export function compareDescribeContract(oracle, envelope) {
  if (oracle?.oracle?.commit !== GO_COMMIT) throw new Error('describe-contract.wrong_oracle');
  validateCommands(oracle.commands);
  const report = {schema: 'lycheedev.executed-describe-contract.v1', oracle: oracle.oracle,
    scope: 'Executed public command names and exact ordered metadata; no handler, parser execution, Skill, mailbox, performance or game parity claim',
    state: 'fail', rawExactMatch: false, acceptedContractMatch: false,
    approvedEngineSummaryChanges: []};
  const actual = envelope?.result?.commands;
  try {
    if (envelope?.schema !== 'lycheedev.result.v1' || envelope.ok !== true || envelope.error !== null) throw new Error('invalid successful describe envelope');
    validateCommands(actual);
    for (const command of actual) for (const key of Object.keys(command)) {
      if (!['path', ...fields].includes(key)) throw new Error(`unknown command metadata: ${command.path}.${key}`);
    }
  } catch (error) {
    return {...report, invalid: error.message, expectedCount: oracle.commands.length,
      actualCount: Array.isArray(actual) ? actual.length : null};
  }
  const rawDiff = diffByKey(oracle.commands, actual, 'path', fields);
  const actualNames = new Set(actual.map(c => c.path));
  const expectedNames = new Set(oracle.commands.map(c => c.path));
  const expectedOrder = oracle.commands.filter(c => actualNames.has(c.path)).map(c => c.path);
  const actualOrder = actual.filter(c => expectedNames.has(c.path)).map(c => c.path);
  const commandOrder = {matches: JSON.stringify(expectedOrder) === JSON.stringify(actualOrder), expected: expectedOrder, actual: actualOrder};
  const unapprovedChanges = [];
  for (const row of rawDiff.changed) {
    const changes = row.changes.filter(change => {
      const approved = change.field === 'summary' && APPROVED_ENGINE_SUMMARIES.some(allowed =>
        allowed.path === row.path && allowed.expected === change.expected && allowed.actual === change.actual);
      if (approved) report.approvedEngineSummaryChanges.push({path: row.path, ...change});
      return !approved;
    });
    if (changes.length) unapprovedChanges.push({path: row.path, changes});
  }
  const rawExactMatch = rawDiff.state === 'pass' && commandOrder.matches;
  const acceptedContractMatch = !rawDiff.missing.length && !rawDiff.extra.length && !unapprovedChanges.length && commandOrder.matches;
  return {...report, state: acceptedContractMatch ? 'pass' : 'fail', rawExactMatch, acceptedContractMatch,
    rawDiff, commandOrder, unapprovedChanges};
}
