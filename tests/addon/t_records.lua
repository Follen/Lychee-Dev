-- Saved Records page (exports tab): newest record auto-selected with a
-- pre-selected rewrite-guarded ticket box, pending/saved badges, header stats,
-- detail fields (B/KB/MB sizes, client version (build), source path fallback),
-- legacy-kind label fallback, row selection, 36 px wheel step, two-click
-- delete with protected records refused, combat-guarded ReloadUI, two-click
-- cache clear that skips protected records and preserves the monotonic ticket
-- counter, and the empty state strings.
local Env, client, root = ...

local ns = Env.LoadWorkbench()
Env.LoadAddon("UI/Pages/ExportRecords.lua", ns)
assert(ns.Persistence.Load(), "persistence did not load")
assert(ns.Stores.Initialize(), "stores did not initialize")

local L = ns.L
local W = ns.Workbench
local Exports = ns.Stores.Exports

-- The page registers at load time and creates no frames before the first /dev.
assert(Env.framesCreated == 0, "Saved Records page created frames before /dev was used")

-- Seed records: 12 plain records (the oldest with a source path), legacy
-- performance evidence, an automation result, an unknown legacy kind (saved,
-- not pending), size-boundary payloads and one protected record (newest,
-- pending). Store contract: options.protect keeps a record out of delete and
-- clear until its reload disk write (MarkSaved) completes.
local tickets = {}
for index = 1, 12 do
    local id = assert(Exports.Add("test", "record " .. index, "record payload " .. index,
        index == 1 and { path = "Test.First" } or nil))
    tickets[#tickets + 1] = id
end
local legacyTicket = assert(Exports.Add("performance_capture", "Legacy performance",
    "retained legacy payload"))
local automationTicket = assert(Exports.Add("automation_result", "Automation result",
    "automation payload"))
local unknownTicket = assert(Exports.Add("legacy_blob", "Unknown legacy", "unknown payload"))
assert(Exports.MarkSaved(unknownTicket), "seed record could not be marked saved")
local kbTicket = assert(Exports.Add("run_result", "Two KB", string.rep("x", 2048)))
local mbTicket = assert(Exports.Add("run_result", "Two MB", string.rep("x", 2 * 1024 * 1024)))
local protectedTicket = assert(Exports.Add("run_result", "Protected result", "protected payload",
    { path = "Test.Path" }, { protect = true }))

local recordCount = #Exports.GetOrder()
assert(recordCount == 18, "seed data did not create 18 records")

-- First /dev opens the workbench; the records page builds on first activation.
assert(W.ShowPage("exports"), "Saved Records page did not activate")
local page = assert(W.GetPage("exports"), "Saved Records page was not built")
assert(page:IsShown(), "Saved Records page did not build a visible page frame")

-- Header stats and row chrome.
local statCount, statBytes, statMaximum = Exports.GetStats()
assert(statCount == recordCount, "export stats did not count the seeded records")
assert(page.countText:GetText() == string.format(L.EXPORT_RECORD_COUNT, recordCount),
    "record count stat was wrong")
assert(page.usageText:GetText() == string.format(L.EXPORT_STORAGE_USAGE,
        statBytes / 1024, statMaximum / 1024 / 1024),
    "storage usage stat was wrong")
assert(page.pendingText:GetText() == string.format(L.EXPORT_PENDING_COUNT, recordCount - 1),
    "pending count stat was wrong")
assert(#page.rows == recordCount, "record list did not build one row per record")
assert(page.rows[1]:GetHeight() == 58, "record rows did not use the 58 px row height")
assert(page.rows[1].background and page.rows[1].divider,
    "record rows did not use the aligned flat-row component")

-- Pending / saved badges.
assert(page.rows[1].status:GetText() == L.EXPORT_STATUS_PENDING,
    "pending record did not show the pending badge")
local unknownRow
for index = 1, #page.rows do
    if page.rows[index].ticket == unknownTicket then
        unknownRow = page.rows[index]
    end
end
assert(unknownRow and unknownRow.status:GetText() == L.EXPORT_STATUS_SAVED,
    "saved record did not show the saved badge")

-- Newest record is auto-selected with a pre-selected ticket.
assert(page:GetSelectedTicket() == Exports.GetOrder()[1] and Exports.GetOrder()[1] == protectedTicket,
    "newest record was not auto-selected")
assert(page.ticketBox:GetText() == protectedTicket, "ticket box did not show the selected ticket")
assert(page.ticketBox.focused and page.ticketBox.highlighted, "ticket box was not pre-selected")
assert(page.ticketHint:GetText() == L.EXPORT_TICKET_HELP, "ticket box did not show the copy hint")
assert(page.detailStatus:GetText() == L.EXPORT_STATUS_PENDING,
    "detail pane did not show the selected record badge")

-- Row meta: time | kind | size.
assert(page.rows[1].meta:GetText() == os.date("%Y-%m-%d %H:%M:%S", Env.now) .. " | "
        .. L.EXPORT_KIND_RUN_RESULT .. " | " .. string.format("%d B", #"protected payload"),
    "record row meta line was wrong")

-- Detail fields.
local clientVersion, clientBuild = GetBuildInfo()
assert(page.nameValue:GetText() == "Protected result"
        and page.kindValue:GetText() == L.EXPORT_KIND_RUN_RESULT,
    "detail name/kind fields were wrong")
assert(page.timeValue:GetText() == os.date("%Y-%m-%d %H:%M:%S", Env.now),
    "detail time field did not use %Y-%m-%d %H:%M:%S")
assert(page.clientValue:GetText() == clientVersion .. " (" .. clientBuild .. ")",
    "detail client field was wrong")
assert(page.pathValue:GetText() == "Test.Path", "detail path field was wrong")

-- Size formatting: B / KB / MB.
page.SelectRecord(kbTicket)
assert(page.sizeValue:GetText() == "2.0 KB", "2 KB payload was not formatted as KB")
page.SelectRecord(mbTicket)
assert(page.sizeValue:GetText() == "2.00 MB", "2 MB payload was not formatted as MB")
page.SelectRecord(unknownTicket)
assert(page.sizeValue:GetText() == "15 B", "byte payload was not formatted as B")

-- Kind labels incl. the legacy-kind fallback.
assert(page.kindValue:GetText() == L.EXPORT_KIND_UNKNOWN,
    "unknown legacy kind did not fall back to the generic label")
page.SelectRecord(legacyTicket)
assert(page.kindValue:GetText() == L.EXPORT_KIND_PERFORMANCE,
    "legacy performance kind label was not preserved")
page.SelectRecord(automationTicket)
assert(page.kindValue:GetText() == L.EXPORT_KIND_AUTOMATION_RESULT,
    "automation_result kind label was missing")

-- Missing source path falls back to NOT_AVAILABLE.
assert(page.pathValue:GetText() == L.NOT_AVAILABLE, "missing source path was not reported")

-- List wheel scroll (36 px step).
Env.FireScript(page.listScroll, "OnMouseWheel", -1)
assert(page.listScroll:GetVerticalScroll() == 36, "record list wheel did not step 36 px")

-- Row selection.
page.rows[2]:Click()
assert(page:GetSelectedTicket() == page.rows[2].ticket
        and page.ticketBox:GetText() == page.rows[2].ticket,
    "record rows could not be selected")

-- Ticket rewrite guard + re-highlight.
page.SelectRecord(unknownTicket)
assert(page.ticketBox.savedText == unknownTicket, "ticket box did not remember the saved ticket")
page.ticketBox:SetText("manual edit")
assert(page.ticketBox:GetText() == unknownTicket,
    "ticket box accepted a manual edit instead of rewriting")
assert(page.ticketBox.highlighted, "ticket box did not re-highlight after the rewrite")

-- Delete record: two-click confirm, then delete.
local countBeforeDelete = Exports.GetStats()
page.deleteButton:Click()
assert(page.deleteButton.label:GetText() == L.EXPORT_RECORD_DELETE_CONFIRM
        and page.deleteButton.variant == "danger",
    "record deletion did not require confirmation")
assert(Exports.GetStats() == countBeforeDelete, "first delete click already deleted")
page.deleteButton:Click()
assert(Exports.Get(unknownTicket) == nil and Exports.GetStats() == countBeforeDelete - 1,
    "selected export record was not deleted")
assert(page:GetSelectedTicket() == Exports.GetOrder()[1],
    "selection did not fall back to the newest record after delete")

-- Protected records survive delete until their reload disk write.
page.SelectRecord(protectedTicket)
page.deleteButton:Click()
page.deleteButton:Click()
assert(Exports.Get(protectedTicket) ~= nil and Exports.IsProtected(protectedTicket),
    "protected record was deleted")

-- Reload UI: combat-guarded ReloadUI().
assert(page.reloadButton:IsEnabled(), "reload action was disabled with pending records")
Env.reloadCalled = false
page.reloadButton:Click()
assert(Env.reloadCalled, "reload action did not call ReloadUI")
Env.reloadCalled = false
Env.SetInCombat(true)
page.reloadButton:Click()
assert(not Env.reloadCalled, "reload action called ReloadUI during combat")
local countBeforeCombatClear = Exports.GetStats()
page.clearButton:Click()
page.clearButton:Click()
assert(Exports.GetStats() == countBeforeCombatClear,
    "clear cache ran its destructive action during combat")
Env.SetInCombat(false)

-- Clear cache: two-click confirm, skips protected, keeps the ticket counter,
-- clears the pending state of everything it removes.
local nextIdBeforeClear = ns.Stores.Initialize().exports.nextId
local pendingBeforeClear = Exports.GetPendingCount()
assert(page.clearButton:IsEnabled(), "nonempty export cache could not be cleared")
page.clearButton:Click()
assert(page.clearButton.label:GetText() == L.CONFIRM_CLEAR_CACHE
        and page.clearButton.variant == "danger",
    "export cache clear did not require confirmation")
assert(Exports.GetStats() == countBeforeCombatClear, "first clear click already cleared")
page.clearButton:Click()
assert(Exports.GetStats() == 1 and Exports.Get(protectedTicket) ~= nil,
    "export cache clear did not skip the protected record")
assert(ns.Stores.Initialize().exports.nextId == nextIdBeforeClear,
    "export cache clear changed the ticket sequence")
assert(Exports.GetPendingCount() == pendingBeforeClear - 16,
    "export cache clear did not drop the pending state of removed records")
assert(page:GetSelectedTicket() == protectedTicket, "selection did not survive the cache clear")

-- The protected record's own pending state clears with its reload disk write.
assert(Exports.MarkSaved(protectedTicket), "protected record could not be marked saved")
page.Refresh()
assert(Exports.GetPendingCount() == 0 and page.pendingText:GetText() == "",
    "saved records kept a pending disk-write state")
assert(not page.reloadButton:IsEnabled(), "reload action stayed enabled without pending records")

-- Tickets stay monotonic and are never reused.
local afterClearTicket = assert(Exports.Add("run_result", "After clear", "new export"))
assert(afterClearTicket ~= legacyTicket and afterClearTicket ~= unknownTicket
        and afterClearTicket ~= protectedTicket,
    "clearing exports reused an old ticket")
assert(afterClearTicket:match("^LYCHEE%-%d%d%d%d%d%d%d%d%-%d%d%d%d%d%d%-%d%d%d%d$"),
    "ticket id format changed")

-- Empty state strings once the last records are gone.
assert(Exports.Delete(protectedTicket), "saved record could not be deleted")
assert(Exports.Delete(afterClearTicket), "last record could not be deleted")
page.Refresh()
assert(Exports.GetStats() == 0, "records survived test cleanup")
assert(page.emptyTitle:IsShown() and page.emptyHelp:IsShown(),
    "empty state strings were not shown for an empty store")
assert(page.emptyTitle:GetText() == L.EXPORT_RECORDS_EMPTY
        and page.emptyHelp:GetText() == L.EXPORT_RECORDS_EMPTY_HELP,
    "empty state strings were wrong")
assert(not page.clearButton:IsEnabled(), "empty export cache could be cleared")
assert(page.nameValue:GetText() == "" and page.kindValue:GetText() == ""
        and page.ticketBox:GetText() == "",
    "detail pane kept stale values for an empty store")

-- Window hide disarms both two-click confirmations.
local lastTicket = assert(Exports.Add("run_result", "Last", "last payload"))
page.Refresh()
page.deleteButton:Click()
assert(page.deleteButton.label:GetText() == L.EXPORT_RECORD_DELETE_CONFIRM,
    "delete confirmation did not arm")
page.clearButton:Click()
assert(page.clearButton.label:GetText() == L.CONFIRM_CLEAR_CACHE,
    "clear confirmation did not arm")
W.Close()
assert(page.deleteButton.label:GetText() == L.EXPORT_RECORD_DELETE
        and page.clearButton.label:GetText() == L.CLEAR_EXPORT_CACHE,
    "shutdown did not disarm the two-click confirmations")

print("Saved Records page tests passed [" .. client .. "]")
