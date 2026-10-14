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
  go('BASE-08', 'Mailbox input preserves focused text and rejects Esc', 'internal/live/channel', 'TestCurrentProtocolRejectsEscapesAndLegacyInput', 'TestFocusedInputWaitsWithoutEscapeAcrossResume'),
  go('BASE-09', 'Reload uses its distinct function key without chat', 'internal/desktop', 'TestMailboxReloadUsesDistinctFunctionKeyWithoutChat'),
  go('BASE-10', 'Independent bootstrap and bounded upgrade evidence', 'internal/live/memory', 'TestBootstrapRemainsReadableWithoutBusinessMailbox', 'TestPreviousProtocolPublishesUpgradeEvidenceOnly'),
  go('BASE-11', 'Transient mailbox reads retain the original goal and errors', 'internal/live/channel', 'TestMailboxTransientObservationContinuesOriginalGoal', 'TestMailboxPermanentReadFailureStillStops'),
  go('BASE-12', 'Native recovery never replays business input', 'tests/protocol', 'TestSlotDriverLuaRecoveryWithoutBusinessReplay', 'TestMemoryRecordLuaParity'),
  go('BASE-13', 'Native input observation and receiver bindings', 'tests/protocol', 'TestInputStateObservation', 'TestLuaReceiverBindings'),
  go('BASE-14', 'Native connection close and crash ownership recovery', 'internal/live/channel', 'TestClosedProjectRetiresClaimAfterCrashWithoutGame', 'TestPendingCloseDoesNotInheritBoundCompletion'),
  go('BASE-15', 'Client discovery fixes installation and process identity', 'internal/live', 'TestDiscoveredClientWindowInfersInstallationAndOptionalPID', 'TestDiscoveredClientWindowGameRootUsesPIDWithoutRetargeting', 'TestDiscoveredClientWindowAmbiguityDoesNotUseTitle'),
  go('BASE-16', 'Window ownership survives interrupted admission', 'internal/live/journal', 'TestWindowAdmissionCrashRecovery', 'TestFailedIntentCannotPublishWindowClaim'),
  go('BASE-17', 'Workbench pages, native bindings, disabled cost and locale fixtures', 'tests/addon', 'TestWorkbenchLuaSuites', 'TestUnifiedTOCContract'),
  go('BASE-18', 'Evidence integrity and isolated workspaces', 'internal/vault', 'TestBlobRoundTripAndCorruption', 'TestTwoHomesWithUnicodeAndSpacePathsAreIsolated'),
  go('BASE-19', 'SQL partial coverage, effective hotfix provenance and JSONL completion', 'internal/records/navigatetest', 'TestSQLIdentityLookupPreservesPartialCoverage', 'TestExplicitEffectiveSQLKeepsBaseAndProvenance', 'TestStreamEmitsTypedFrameContract', 'TestStreamErrorFrameEndsStreamWithoutEnd'),
  go('BASE-20', 'SQL aggregation and subquery semantics', 'internal/records/relational', 'TestGroupSubqueries', 'TestAggregateInputSubqueries', 'TestSubqueryCardinality'),
  go('BASE-21', 'Update preflight, replacement and interrupted recovery', 'internal/delivery', 'TestUpdateManagedPlanReplaceRepeat', 'TestUpdatePreflightsAllBeforeFirstMutation', 'TestUpdateRecoversOldMovedCheckpoint', 'TestUpdateRecoversInterruptedOldFileDeletion', 'TestUpdateRejectsChangedAndContainedTargets'),
];
// These are real-client checks, not satisfied by four-profile Lua fixtures.
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
