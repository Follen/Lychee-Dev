local ADDON_NAME, ns = ...

-- Automation task registry. This file contains generated data only; loading it
-- must never execute task source. Task blocks are written between the
-- BEGIN/END markers by the Lychee Dev skill tooling and are removed before
-- release packaging ships an empty registry.

ns.AutomationTaskDefinitions = ns.AutomationTaskDefinitions or {}

-- Task blocks use exactly this marker shape (task id may not contain the
-- marker text): "-- BEGIN LYCHEE DEV TASK <task-id>" ... "-- END LYCHEE DEV TASK <task-id>".
-- The skill tooling inserts and replaces them; releases ship this file empty.
