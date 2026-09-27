// Stable acceptance IDs. Exact Go test names prevent a renamed/deleted test
// from silently turning a functional baseline into an empty successful run.
const go = (id, title, pkg, ...tests) => ({ id, title, kind: 'go', package: pkg, tests });
export const offlineCases = [
  go('BASE-01', 'Command admission and passive discovery', 'internal/command', 'TestLiveCommandFlagAdmissionComesFromItsContract', 'TestPassiveDiscoveryParsesWithoutDowngradingToActive', 'TestExecuteContractRejectsIncompleteRequestsBeforeWorkspaceAccess'),
  go('BASE-02', 'Pinned targets and concurrent selection', 'internal/selection', 'TestPinsAreStableDerivedAndIsolated', 'TestConcurrentResolutionsDoNotLeakPins'),
  go('BASE-03', 'Source checkout isolation and dirty detection', 'internal/codebase', 'TestWorktreeExactReuseAndDirtyDetection', 'TestWorktreeConcurrentSameCommit', 'TestPruneSourceWorktreesKeepsActiveLeaseAndPins'),
  go('BASE-04', 'Source semantic environment and cache identity', 'internal/codebase', 'TestSemanticDefinitionsReuseFixedEnvironment', 'TestSemanticCacheBindsRuntimeEnvironmentAndSource'),
  go('BASE-05', 'Hotfix pagination and missing-key integrity', 'internal/records', 'TestWagoHotfixMultiPageWithIdentityKeys', 'TestMissingBytesCannotBecomeZeroValues', 'TestKeySourcesAreExplicitBoundedAndOffline'),
  go('BASE-06', 'Asset evidence and failed export preservation', 'internal/records', 'TestExportAssetEvidenceAndFailurePreservation'),
  go('BASE-07', 'Clean managed install and upgrade', 'internal/delivery', 'TestInstallAddonFirstInstallAndRetry', 'TestInstallAddonRefusesModifiedManagedInstallation', 'TestUpgradeAddonResumeAndRemovePreserveOwnedState'),
  go('BASE-08', 'Receiver correlation, partial construction and focus release', 'tests/protocol', 'TestLuaReceiverCorrelationAndInputFaults', 'TestLuaReceiverRollsBackPartialUI', 'TestLuaReceiverBindings'),
  go('BASE-09', 'Physical commit and fixed reload intent before input', 'internal/desktop', 'TestReceiverCommitEndsWithOneEnterAndStopsBeforeItOnGuardFailure', 'TestFixedReloadRecordsBeforeEveryStep'),
  go('BASE-10', 'Reload beacon bounded lifetime and fresh rotation', 'tests/protocol', 'TestLuaStartupBeacon'),
  go('BASE-11', 'Beacon evidence and bounded wake', 'internal/live', 'TestStartupBeaconRequiresFreshRotation', 'TestReceiverWakeSurvivesLoadingWithoutResendingBusiness', 'TestReceiverWakeBoundAndFailure'),
  go('BASE-12', 'Async resources, unobstructed execution and character storage', 'tests/protocol', 'TestFourClientProbeExecution', 'TestInvestigationOwnsResourcesThroughFinalization', 'TestCharacterBridgeStorageDoesNotImportAccountExecution'),
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
// These are real-client checks, not satisfied by four-profile Lua fixtures.
export const liveCases = [
  { id: 'LIVE-01', title: 'Synchronous result through complete cleanup', file: 'sync.lua', budget: 10 },
  { id: 'LIVE-02', title: 'Same request and completed resume reuse the exact evidence' },
  { id: 'LIVE-03', title: 'Expected business failure still finishes cleanup', file: 'failure.lua', budget: 10, expectedFailure: true },
  { id: 'LIVE-04', title: '55-second async execution, nine pages and no transport overlay', file: 'workbench.lua', budget: 60 },
];
export const manualCases = [
  ['REAL-01', 'WGC visual review: font, rounded corners, navigation, settings and receiver'],
  ['REAL-02', 'Chinese/English, small viewport and UI scaling'],
  ['REAL-03', 'Fixed shortcuts, receiving input shield, IME, chat and focus release'],
  ['REAL-04', 'Reload RGB rotation, wake disappearance and 45-second expiry'],
  ['REAL-05', 'First installation, upgrade, relogin and combat transitions'],
  ['REAL-06', 'Two live windows, characters, workspaces and client builds'],
  ['REAL-07', 'Classic 50504 production execution and finish'],
  ['REAL-08', 'Titan 38002 production execution and finish'],
  ['REAL-09', 'Long-running and adversarial schedule acceptance'],
].map(([id, title]) => ({ id, title, kind: 'manual', state: 'not_run' }));
