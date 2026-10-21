// Current CLI metadata is inherited only after authenticating the fixed Go
// 3.2.4 sources. This reuses the reviewed adapter, not a historical result.
import assert from 'node:assert/strict';
import { sha256 } from './oracle.mjs';
import { loadCliContractOracle as loadInherited, verifyGo323CommandInheritance,
  HISTORICAL_FIXTURE_SHA256, RETIRED_DATA_COMMANDS } from './cli-contract-oracle-3.2.3.mjs';
export { HISTORICAL_FIXTURE_SHA256, RETIRED_DATA_COMMANDS };

export const GO_324 = Object.freeze({
  release: 'Go v3.2.4', commit: '29bdbdaff5f064ec212ff2064380b1ccac652eda',
});
export const GO_324_SOURCES = Object.freeze({
  commands: Object.freeze({path: 'internal/command/command_contract.go',
    sha256: 'b56c73427279a89196dcdebc720827e4eb4bc79f55f0d76f7561d49f69eed9fa'}),
  products: Object.freeze({path: 'internal/selection/products.go',
    sha256: '53921c7bf0adcfde2b39c467ec480a80e70c1e1fa562edc2450c75f19a1d9bb2'}),
});
export function loadCliContractOracle(historicalBytes) {
  const inherited = loadInherited(historicalBytes);
  return {...inherited, oracle: GO_324, inheritedMetadataReference: inherited.oracle};
}
export function verifyGo324CommandInheritance(sources, historicalBytes) {
  for (const name of ['commands', 'products']) {
    assert.equal(sha256(sources[name]), GO_324_SOURCES[name].sha256, `Go 3.2.4 ${name} source identity`);
  }
  const inherited = verifyGo323CommandInheritance(sources, historicalBytes);
  return {...inherited, oracle: GO_324, sources: GO_324_SOURCES,
    inheritedMetadataReference: inherited.oracle};
}
