// Check returned proof paths against independently published graph facts.
// This verifier neither runs nor imports the Rust projector or old flow binder.
import assert from 'node:assert/strict';
import {isDeepStrictEqual as equal} from 'node:util';

const key = node => JSON.stringify(node);

export function verifyFlowScenario(spec, graph, flow) {
  const host = graph.host_runtime, control = graph.native_invocations.control;
  const originals = new Map(spec.originals.map(row => [row.path, Buffer.from(row.text)]));
  const calls = new Map(host.calls.map(row => [row.call, row]));
  const expressionResults = new Map(host.expressions.map(row => [row.result, row]));
  const writeJoins = new Map(control.write_joins.map(row => [row.destination, row]));
  const scalarResults = new Map(host.calls.map(row => [row.scalar_result, row]));
  const conditionalResults = new Map(host.transfers.filter(row => row.return_candidates)
    .map(row => [row.destination.use_fact, row.return_candidates]));
  let proofs = 0;

  function site(value) {
    const bytes = originals.get(value.file);
    assert(bytes && value.start >= 0 && value.end > value.start && value.end <= bytes.length,
      `${spec.id}: original byte range`);
    const slice = bytes.subarray(value.start, value.end);
    assert.equal(Buffer.from(slice.toString('utf8')).length, slice.length, `${spec.id}: UTF8 range`);
    return slice.toString('utf8');
  }
  function compatible(predicates) {
    const seen = new Map();
    for (const predicate of predicates) {
      assert(graph.uses[predicate.condition], `${spec.id}: predicate use exists`);
      assert(!seen.has(predicate.condition) || seen.get(predicate.condition) === predicate.outcome,
        `${spec.id}: contradictory proof path`);
      seen.set(predicate.condition, predicate.outcome);
    }
  }
  function fact(node) {
    if (node.kind === 'use') return graph.uses[node.use_fact];
    if (node.kind === 'write') return graph.writes[node.write];
    const call = calls.get(node.call), list = node.kind === 'argument' ? call.arguments : call.returns;
    return {...graph.calls[node.call], decision: list[node.slot - 1].selected,
      writes: list[node.slot - 1].writes};
  }
  function edge(from, to, candidate) {
    if (to.kind === 'argument' || to.kind === 'return') {
      const slots = to.kind === 'argument' ? calls.get(to.call).arguments : calls.get(to.call).returns;
      return from.kind === 'use' && slots[to.slot - 1].use_fact === from.use_fact;
    }
    if (to.kind === 'write') {
      const joined = writeJoins.get(to.write);
      if (joined) return from.kind === 'write' && joined.candidates.some(row =>
        row.write === from.write && row.id === candidate);
      return from.kind === 'use' && graph.writes[to.write].inputs.includes(from.use_fact);
    }
    const result = conditionalResults.get(to.use_fact);
    if (result) return from.kind === 'use' && result.some(row =>
      row.id === candidate && row.source_uses.includes(from.use_fact));
    const scalar = scalarResults.get(to.use_fact);
    if (scalar) return from.kind === 'return' && from.call === scalar.call && from.slot === 1;
    const expression = expressionResults.get(to.use_fact);
    if (expression) return from.kind === 'use' &&
      ['parenthesis', 'selected_operand', 'and', 'or'].includes(expression.operator) &&
      expression.operands.includes(from.use_fact);
    return from.kind === 'write' && graph.uses[to.use_fact].writes.includes(from.write);
  }
  function proof(candidate, sink) {
    proofs++;
    compatible(candidate.conditions);
    const {seed, steps} = candidate, source = calls.get(seed.call);
    assert.equal(source.host_binding, seed.binding, `${spec.id}: actual source binding`);
    const binding = host.bindings[seed.binding].declaration;
    assert.equal(binding.rules.record.returns[String(seed.slot)].state, 'secret');
    assert.equal(source.returns[seed.slot - 1].selected.detail, seed.value);
    assert(equal(seed.rule, binding.rules.evidence));
    assert.equal(seed.rule_pointer, `${binding.rules.evidence.pointer}/returns/${seed.slot}`);
    assert(steps.length >= 1);
    assert.equal(steps[0].node.kind, 'use');
    assert.equal(steps[0].node.use_fact, source.returns[seed.slot - 1].use_fact);
    if (sink) assert(equal(steps.at(-1).node, sink));
    for (const [index, step] of steps.entries()) {
      const actual = fact(step.node);
      assert(actual, `${spec.id}: proof node exists`);
      assert(equal(step.site, actual.site) && step.context === actual.context);
      assert(equal(step.selected, actual.decision));
      site(step.site);
      const observation = control.context_observations[step.context];
      assert.equal(step.occurrence, observation.occurrence);
      assert(!observation.view || observation.view.kind !== 'source_template');
      if (index) assert(edge(steps[index - 1].node, step.node, steps[index - 1].candidate),
        `${spec.id}: no invented proof edge ${key(steps[index - 1].node)} -> ${key(step.node)}`);
    }
  }

  assert.equal(flow.input_complete, true);
  assert.equal(flow.complete, spec.expected.complete, `${spec.id}: completion`);
  assert.equal(flow.truncated, spec.expected.truncated, `${spec.id}: truncation`);
  assert.equal(flow.findings.has_more, false, `${spec.id}: default page covers fixture`);
  const unique = new Set(flow.findings.rows.map(row => `${row.call}:${row.argument}`));
  assert.equal(unique.size, flow.findings.rows.length, `${spec.id}: duplicate sink occurrence`);
  if (spec.expected.multiplicity === 'reachable_prefix') assert(unique.size >= spec.expected.findings);
  else assert.equal(unique.size, spec.expected.findings, `${spec.id}: sink occurrence count`);
  if (spec.expected.complete) assert.equal(flow.gaps.length, 0, `${spec.id}: complete has no unresolved gap`);
  else assert(flow.gaps.length > 0, `${spec.id}: exact uncertainty is visible`);
  for (const finding of flow.findings.rows) {
    const call = calls.get(finding.call), binding = host.bindings[call.host_binding].declaration;
    assert.equal(binding.rules.record.arguments[String(finding.argument)].secret, 'forbidden');
    assert.equal(finding.argument, spec.expected.forbiddenSlot);
    assert(equal(finding.site, graph.calls[finding.call].site));
    assert(finding.candidates.length > 0);
    for (const candidate of finding.candidates) proof(candidate,
      {kind: 'argument', call: finding.call, slot: finding.argument});
  }
  for (const guard of flow.guarded) {
    const predicate = calls.get(guard.guard_call);
    assert.equal(host.bindings[predicate.host_binding].declaration.kind, 'predicate');
    assert.equal(predicate.predicate.subject.use_fact, guard.subject_use);
    assert(equal(guard.candidate.seed, guard.subject.seed));
    proof(guard.candidate); proof(guard.subject);
    compatible(guard.conditions);
    const condition = guard.candidate.conditions.find(row => row.condition === guard.condition);
    assert(condition, `${spec.id}: guard condition belongs to candidate path`);
    let use = condition.condition, truth = condition.outcome;
    while (use !== predicate.scalar_result) {
      const expression = expressionResults.get(use);
      assert(expression && ['not', 'parenthesis', 'selected_operand'].includes(expression.operator),
        `${spec.id}: guard must follow real polarity expressions`);
      if (expression.operator === 'not') truth = !truth;
      use = expression.operands[0];
    }
    assert.equal(truth, false, `${spec.id}: only predicate false can remove secret risk`);
  }
  if (spec.expected.guarded) assert(flow.guarded.length > 0, `${spec.id}: guard proof required`);
  if (spec.expected.operator) {
    const expressions = host.expressions.filter(row => row.operator === spec.expected.operator);
    assert(expressions.length > 0, `${spec.id}: actual unsupported expression`);
    assert(flow.gaps.some(gap => gap.site && expressions.some(row => equal(row.site, gap.site)) &&
      gap.dependencies.length > 0), `${spec.id}: operator gap retains Source dependency`);
  }
  if (spec.id === 'F35') {
    assert.equal(flow.statements, 1);
    assert(!host.calls.some(row => host.bindings[row.host_binding]?.declaration.name === 'Use'),
      'F35: Use must not execute before a result is filtered');
  }
  if (spec.id === 'F29') {
    const iterations = flow.findings.rows.map(row => control.occurrences[row.occurrence].loops.at(-1).iteration);
    assert.deepEqual(iterations.sort(), [0, 1]);
  }
  if (spec.id === 'F02') {
    assert(flow.findings.rows.every(row => row.candidates.every(candidate => candidate.seed.slot === 2)));
  }
  if (spec.id === 'F32') {
    const line = value => originals.get(value.file).subarray(0, value.start).toString('utf8').split(/\r\n|\r|\n/).length;
    assert(flow.findings.rows.every(row => line(row.site) === 3 && row.candidates.every(candidate => line(candidate.seed.site) === 2)));
  }
  if (spec.id === 'F33') {
    assert.equal(host.calls.filter(row => row.host_binding !== null &&
      host.bindings[row.host_binding].declaration.name === 'Source').length, 0,
      'F33: comment/string content does not create Source calls');
  }
  return {case: spec.id, findings: unique.size, proofs, complete: flow.complete};
}
