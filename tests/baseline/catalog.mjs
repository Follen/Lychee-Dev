// Stable acceptance IDs. Exact Go test names prevent a renamed/deleted test
// from silently turning a functional baseline into an empty successful run.
const go = (id, title, pkg, ...tests) => ({ id, title, kind: 'go', package: pkg, tests });
export const offlineCases = [
  go('BASE-01', 'Duplex command admission and retired-input rejection', 'internal/command', 'TestLiveCommandFlagAdmissionComesFromItsContract', 'TestDuplexOnlyDirectoryRejectsRetiredInput', 'TestDuplexProbeFileCapacity'),
  go('BASE-02', 'Pinned targets and concurrent selection', 'internal/selection', 'TestPinsAreStableDerivedAndIsolated', 'TestConcurrentResolutionsDoNotLeakPins'),
  go('BASE-03', 'Source checkout isolation and dirty detection', 'internal/codebase', 'TestWorktreeExactReuseAndDirtyDetection', 'TestWorktreeConcurrentSameCommit', 'TestPruneSourceWorktreesKeepsActiveLeaseAndPins'),
  go('BASE-04', 'Source semantic environment and cache identity', 'internal/codebase', 'TestSemanticDefinitionsReuseFixedEnvironment', 'TestSemanticCacheBindsRuntimeEnvironmentAndSource'),
  go('BASE-05', 'Hotfix pagination and missing-key integrity', 'internal/records', 'TestWagoHotfixMultiPageWithIdentityKeys', 'TestMissingBytesCannotBecomeZeroValues', 'TestKeySourcesAreExplicitBoundedAndOffline'),
  go('BASE-06', 'Asset evidence and failed export preservation', 'internal/records', 'TestExportAssetEvidenceAndFailurePreservation'),
  go('BASE-07', 'Clean managed install and upgrade', 'internal/delivery', 'TestInstallAddonFirstInstallAndRetry', 'TestInstallAddonRefusesModifiedManagedInstallation', 'TestUpgradeAddonResumeAndRemovePreserveOwnedState'),
  go('BASE-08', 'Mailbox v1 one-write integrity, durable execution and recovery', 'internal/live/duplex', 'TestWireBoundariesAndTampering', 'TestExecuteOneWriteAndRetainedResult', 'TestUnknownCommandRecoversExactTerminalWithoutReplay', 'TestUnknownUnacceptedCommandNeverReplays', 'TestPersistenceBeforeEffectAndResultACK', 'TestStopUncertainIntentSerializesCancelAndClose', 'TestRuntimeChangeRetainsUnknownEvidence', 'TestSequentialCommandsBoundedCurrentState', 'TestFileStoreDurableRestartAndCorruptEvidence', 'TestIdleRepairRequiresDrainAndDoesNotReplayHistory', 'TestFileStoreInspectionRestoresCompactedCommandWithoutLease', 'TestValidationUsesTransferDeadlineNotExecutionBudget'),
  go('BASE-09', 'Verified migration of idle legacy LoD files and refusal of unresolved pools', 'internal/delivery', 'TestAddonInstallDoesNotCreateLoDSlotPool', 'TestAddonInstallRequiresUpgradeWhenManagedLegacyPoolExists', 'TestAddonUpgradeArchivesIdleManagedSlots', 'TestAddonUpgradeBlocksPendingOrModifiedLegacySlots', 'TestLegacySlotMigrationArchivesOnlyVerifiedIdlePool', 'TestLegacySlotMigrationBlocksPendingModifiedAndUnknownContent', 'TestLegacySlotMigrationResumesPartialDirectoryMoves'),
  go('BASE-15', 'Read-only client installation and native window inventory', 'internal/live', 'TestDiscoveryIsReadOnlyInstallationAndWindowInventory', 'TestDiscoveryKeepsUnsupportedClientPlatformInventory', 'TestClientWindowSelection', 'TestDiscoveredClientWindowAmbiguityDoesNotUseTitle'),
  go('BASE-16', 'Process crash and durable window ownership', 'internal/live/journal', 'TestWindowAdmissionCrashRecovery', 'TestWindowRunOwnershipAndTerminalRelease'),
  go('BASE-17', 'Workbench pages, disabled cost and locale fixtures', 'tests/addon', 'TestWorkbenchLuaSuites', 'TestUnifiedTOCContract'),
  go('BASE-18', 'Evidence integrity and isolated workspaces', 'internal/vault', 'TestBlobRoundTripAndCorruption', 'TestTwoHomesWithUnicodeAndSpacePathsAreIsolated'),
  go('BASE-19', 'SQL partial coverage, effective hotfix provenance and JSONL completion', 'internal/records/navigatetest', 'TestSQLIdentityLookupPreservesPartialCoverage', 'TestExplicitEffectiveSQLKeepsBaseAndProvenance', 'TestStreamEmitsTypedFrameContract', 'TestStreamErrorFrameEndsStreamWithoutEnd'),
  go('BASE-20', 'SQL aggregation and subquery semantics', 'internal/records/relational', 'TestGroupSubqueries', 'TestAggregateInputSubqueries', 'TestSubqueryCardinality'),
  go('BASE-21', 'Update preflight, replacement and interrupted recovery', 'internal/delivery', 'TestUpdateManagedPlanReplaceRepeat', 'TestUpdatePreflightsAllBeforeFirstMutation', 'TestUpdateRecoversOldMovedCheckpoint', 'TestUpdateRecoversInterruptedOldFileDeletion', 'TestUpdateRejectsChangedAndContainedTargets'),
];
// These are real-client checks, not satisfied by four-profile Lua fixtures.
export const manualCases = [
  ['REAL-01', 'WGC visual review of the current mailbox request and release view'],
  ['REAL-02', 'Chinese/English, small viewport and UI scaling'],
  ['REAL-03', 'Mailbox request, result, cancellation and terminal release on the real client'],
  ['REAL-04', 'Reload and relogin preserve durable mailbox request evidence'],
  ['REAL-05', 'First installation, upgrade, relogin and combat transitions'],
  ['REAL-06', 'Two live windows, characters, workspaces and client builds'],
  ['REAL-07', 'Classic 50504 production execution and finish'],
  ['REAL-08', 'Titan 38002 production execution and finish'],
  ['REAL-09', 'Long-running and adversarial schedule acceptance'],
  ['REAL-10', 'Native mailbox v1 real-client acceptance (not_run until a real-client run is recorded)'],
].map(([id, title]) => ({ id, title, kind: 'manual', state: 'not_run' }));
