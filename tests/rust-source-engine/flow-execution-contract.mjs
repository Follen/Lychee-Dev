// Business expectations come from the original inputs, not from a flow result.
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {commonFlowContract} from './flow-common-contract.mjs';
import {hostFactsContract} from './host-facts-contract.mjs';

const sha = bytes => createHash('sha256').update(bytes).digest('hex');
const operators = {F22: '+', F23: '>', F24: '..', F25: 'dynamic_index'};
const gapSubjects = {
  F12: 'unknown_load_order', F13: 'unavailable_callee',
  F17: 'unknown_parameter_callee', F18: 'unknown_call_effect',
  F19: 'unknown_call_effect', F26: 'recursive_call',
  F27: 'nontermination', F30: 'unknown_iterator', F36: 'selector_required',
};

function file(path, text) {
  const bytes = Buffer.from(text);
  return {path, text, bytes: bytes.length, sha256: sha(bytes)};
}

export function flowExecutionContract(root) {
  const original = commonFlowContract(root), host = hostFactsContract(root);
  const cases = original.cases.map(row => ({
    ...row,
    expected: {
      findings: row.expectedBusiness.secretUseExecutionOccurrences,
      multiplicity: row.id === 'F27' ? 'reachable_prefix' : 'exact',
      complete: row.expectedBusiness.completion === 'supported_complete',
      truncated: row.id === 'F35',
      operator: operators[row.id] ?? null,
      gapSubject: gapSubjects[row.id] ?? (operators[row.id] ? 'operation_rule' : null),
      guarded: ['F06', 'F31'].includes(row.id),
      forbiddenSlot: row.id === 'F04' ? 2 : 1,
    },
  }));
  assert.deepEqual(cases.map(row => row.id), Array.from({length: 36}, (_, n) => `F${String(n + 1).padStart(2, '0')}`));
  // Exact selectors are independent requests over the unchanged F36 bytes.
  const f36 = cases.at(-1);
  const selectorCases = ['A', 'B'].map(symbol => ({
    ...f36, id: `F36-${symbol}`, originalCase: 'F36',
    target: {...f36.target, symbol, line: undefined},
    entry: {kind: 'abstract_function_entry', path: f36.target.path, symbol, arguments: 'symbolic'},
    expected: {...f36.expected, findings: symbol === 'A' ? 1 : 0, complete: true, gapSubject: null},
  }));
  const extra = [
    ['C01', 'conditional-return-same-observation-guard', 0,
      'local function Choose() if flag then return Source() else return Source() end end\nlocal x=Choose()\nif issecretvalue(x) then return end\nUse(x)\n'],
    ['C02', 'conditional-return-different-observation-guard', 1,
      'local function Choose() if flag then return Source() else return Source() end end\nlocal x=Choose()\nlocal y=Choose()\nif issecretvalue(y) then return end\nUse(x)\n'],
    ['C03', 'new-value-after-guard', 1,
      'local x=Source()\nif issecretvalue(x) then return end\nx=Source()\nUse(x)\n'],
    ['C04', 'capture-reads-current-cell', 0,
      'local x=Source()\nlocal function Get() return x end\nx=nil\nUse(Get())\n'],
    ['C05', 'conditional-write-compatible-paths', 1,
      'local x=Source()\nif flag then x=nil else x=Source() end\nUse(x)\n'],
    ['C06', 'conditional-field-absence', 1,
      'local t={}\nif flag then t.value=Source() end\nUse(t.value)\n'],
    ['C07', 'equal-value-distinct-writes', 0,
      'local x=Source()\nlocal y\nif flag then y=x else y=x end\nif issecretvalue(y) then return end\nUse(y)\n'],
    ['C08', 'captured-conditional-write', 1,
      'local x=Source()\nlocal function Get() return x end\nif flag then x=nil end\nUse(Get())\n'],
  ].map(([id, name, findings, text]) => ({
    id, name, originals: [file('main.lua', text)], target: {path: 'main.lua'},
    entry: {kind: 'ordered_top_level', path: 'main.lua'},
    load: {state: 'explicit', ordered: ['main.lua']}, budget: {},
    expected: {findings, multiplicity: 'exact', complete: true, truncated: false,
      operator: null, gapSubject: null, guarded: ['C01', 'C07'].includes(id), forbiddenSlot: 1},
  }));
  extra.push({
    id: 'C09', name: 'unknown-entry-argument-at-authenticated-sink',
    originals: [file('main.lua', 'function Demo(value)\n Use(value)\nend\n')],
    target: {path: 'main.lua', symbol: 'Demo'},
    entry: {kind: 'abstract_function_entry', path: 'main.lua', symbol: 'Demo', arguments: 'symbolic'},
    load: {state: 'explicit', ordered: ['main.lua']}, budget: {},
    expected: {findings: 0, multiplicity: 'exact', complete: false, truncated: false,
      operator: null, gapSubject: 'unknown_parameter_value', guarded: false, forbiddenSlot: 1},
  });
  return {
    schema: 'lycheedev.source-flow-execution-contract.v1',
    originalsSHA256: original.matrixSHA256,
    originalRulesSHA256: sha(Buffer.from(JSON.stringify(original.rules))),
    materials: host.materials, bindings: host.bindings, rules: original.rules,
    originalCases: cases, selectorCases, additionalCases: extra,
    scope: 'Original 36 business cases plus concrete join/guard counterexamples; not production WoW API or full Source acceptance',
  };
}
