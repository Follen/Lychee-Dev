// Stable acceptance IDs. Exact Go test names prevent a renamed/deleted test
// from silently turning a functional baseline into an empty successful run.
const go = (id, title, pkg, ...tests) => ({ id, title, kind: 'go', package: pkg, tests });
const profiles = ['retail', 'classic', 'titan', 'forever', 'classic-era', 'anniversary'];
const lua = (id, title, ...suites) => ({ ...go(id, title, 'tests/addon', ...suites.flatMap(suite => profiles.map(profile => `TestWorkbenchLuaSuites/${suite}/${profile}`))), luaSuites: suites, profiles });
export const offlineCases = [
  go('BASE-01', 'Command admission and passive discovery', 'internal/command', 'TestLiveCommandFlagAdmissionComesFromItsContract', 'TestPassiveDiscoveryParsesWithoutDowngradingToActive', 'TestExecuteContractRejectsIncompleteRequestsBeforeWorkspaceAccess'),
  go('BASE-02', 'Pinned targets and concurrent selection', 'internal/selection', 'TestPinsAreStableDerivedAndIsolated', 'TestConcurrentResolutionsDoNotLeakPins'),
  go('BASE-03', 'Source checkout isolation and dirty detection', 'internal/codebase', 'TestWorktreeExactReuseAndDirtyDetection', 'TestWorktreeConcurrentSameCommit', 'TestPruneSourceWorktreesKeepsActiveLeaseAndPins'),
  go('BASE-04', 'Source semantic environment and cache identity', 'internal/codebase', 'TestSemanticDefinitionsReuseFixedEnvironment', 'TestSemanticCacheBindsRuntimeEnvironmentAndSource'),
  go('BASE-05', 'Hotfix pagination and missing-key integrity', 'internal/records', 'TestWagoHotfixMultiPageWithIdentityKeys', 'TestMissingBytesCannotBecomeZeroValues', 'TestKeySourcesAreExplicitBoundedAndOffline'),
  go('BASE-06', 'Asset evidence and failed export preservation', 'internal/records', 'TestExportAssetEvidenceAndFailurePreservation'),
  go('BASE-07', 'Clean managed install and upgrade', 'internal/delivery', 'TestInstallAddonFirstInstallAndRetry', 'TestInstallAddonRefusesModifiedManagedInstallation', 'TestUpgradeAddonResumeAndRemovePreserveOwnedState'),
  lua('BASE-08', 'Current mailbox correlation, focus ownership and binding gates', 't_input_mailbox.lua', 't_input_binding_gate.lua', 't_focus_bindings.lua'),
  go('BASE-09', 'Physical commit and fixed reload intent before input', 'internal/desktop', 'TestReceiverCommitEndsWithOneEnterAndStopsBeforeItOnGuardFailure', 'TestFixedReloadRecordsBeforeEveryStep'),
  lua('BASE-10', 'Current bootstrap identity, client gate and mailbox readiness', 't_bootstrap.lua', 't_client_gate.lua', 't_bridge_status.lua'),
  go('BASE-11', 'Beacon evidence and bounded wake', 'internal/live', 'TestStartupBeaconRequiresFreshRotation', 'TestReceiverWakeSurvivesLoadingWithoutResendingBusiness', 'TestReceiverWakeBoundAndFailure'),
  lua('BASE-12', 'Current probe resources, durable slot lifecycle and baseline probes', 't_probe_execution.lua', 't_slot_protocol.lua', 't_slot_runtime.lua', 't_baseline_probes.lua'),
  go('BASE-13', 'Verified report, ACK, display proof and immutable budget', 'internal/live', 'TestCompleteInvestigationKeepsOwnerUntilDisplayProof', 'TestExecutionBudgetIsFrozenAcrossResume'),
  go('BASE-14', 'Unknown input and recovery without replay', 'internal/live', 'TestExecuteCompletesLifecycle', 'TestExecuteReconcilesSubmittedInputWithoutReplay', 'TestBootstrapUnknownInputRequiresExplicitAbandon', 'TestBootstrapRestageNeverCrossesCommitFence', 'TestFlushRequiresNewReadinessAndDoesNotReplay', 'TestNextActionDoesNotLoopOnTerminalReportFailure'),
  go('BASE-15', 'Multiple workspaces, windows and character writers', 'internal/live', 'TestBootstrapAdmissionSerializesWorkspacesAndMaintenance', 'TestReportWritersDistinguishProcessesAndCharacterPartitions', 'TestPassiveDiscoveryNeverCapturesOrTypes'),
  go('BASE-16', 'Process crash and durable window ownership', 'internal/live/journal', 'TestWindowAdmissionCrashRecovery', 'TestWindowRunOwnershipAndTerminalRelease'),
  go('BASE-17', 'Workbench pages, settings, disabled cost and locale fixtures', 'tests/addon', 'TestWorkbenchLuaSuites', 'TestUnifiedTOCContract'),
  go('BASE-18', 'Evidence integrity and isolated workspaces', 'internal/vault', 'TestBlobRoundTripAndCorruption', 'TestTwoHomesWithUnicodeAndSpacePathsAreIsolated'),
  go('BASE-19', 'SQL partial coverage, effective hotfix provenance and JSONL completion', 'internal/records/navigatetest', 'TestSQLIdentityLookupPreservesPartialCoverage', 'TestExplicitEffectiveSQLKeepsBaseAndProvenance', 'TestStreamEmitsTypedFrameContract', 'TestStreamErrorFrameEndsStreamWithoutEnd'),
  go('BASE-20', 'SQL aggregation and subquery semantics', 'internal/records/relational', 'TestGroupSubqueries', 'TestAggregateInputSubqueries', 'TestSubqueryCardinality'),
  go('BASE-21', 'Update preflight, replacement and interrupted recovery', 'internal/delivery', 'TestUpdateManagedPlanReplaceRepeat', 'TestUpdatePreflightsAllBeforeFirstMutation', 'TestUpdateRecoversOldMovedCheckpoint', 'TestUpdateRecoversInterruptedOldFileDeletion', 'TestUpdateRejectsChangedAndContainedTargets'),
];
// These are real-client checks, not satisfied by six-profile Lua fixtures.
export const liveCases = [];

export const manualCases = [
  ['REAL-01', 'Real-client visual review: font, rounded corners, navigation, native bindings and activity view'],
  ['REAL-02', 'Chinese/English, small viewport and UI scaling'],
  ['REAL-03', 'Effective Native bindings, EditBox passthrough, IME and focus readiness'],
  ['REAL-05', 'First installation, upgrade, relogin and combat transitions'],
  ['REAL-06', 'Two live windows, characters, workspaces and client builds'],
  ['REAL-07', 'Classic 50504 production execution and cleanup'],
  ['REAL-08', 'Titan 38002 production execution and cleanup'],
  ['REAL-09', 'Long-running and adversarial schedule acceptance'],
].map(([id, title]) => ({ id, title, kind: 'manual', state: 'not_run' }));
