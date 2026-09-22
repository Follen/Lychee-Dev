-- Run page (runner tab): run -> tree + export enabled, run_result evidence
-- carries the full result and metadata.code, no-return runs stay in text mode,
-- history rows restore session live trees and persisted stored trees, the 22
-- character preview cap, text/tree switching, Select Result, Tab indentation,
-- clear-history semantics (input kept), EMPTY_INPUT error data, combat refusal
-- and secret-value display guards.
local Env, client, root = ...

local ns = Env.LoadWorkbench()
Env.LoadAddon("UI/Pages/Run.lua", ns)
assert(ns.Persistence.Load(), "persistence did not load")
assert(ns.Stores.Initialize(), "stores did not initialize")

local L = ns.L
local W = ns.Workbench

-- The page registers at load time and creates no frames before the first /dev.
assert(Env.framesCreated == 0, "Run page created frames before /dev was used")

-- Execution counter around the real executor (the page reads ns.Execute).
local executeCount = 0
local realExecute = ns.Execute
ns.Execute = function(code)
    executeCount = executeCount + 1
    return realExecute(code)
end

-- Persisted history entry: stored tree only (no session live tree).
local persistedCode = "return { saved = 1 }"
local persistedTree = {
    roots = {
        {
            label = "[1]",
            kind = "table",
            value = "table (1)",
            expanded = true,
            loaded = true,
            hasMore = false,
            children = {
                { label = "saved", kind = "boolean", value = "true" },
            },
        },
    },
}
assert(ns.Stores.History.Add(persistedCode, "{ saved = 1 }", true, persistedTree),
    "persisted history entry was not stored")

-- First /dev opens the workbench and builds the runner page lazily.
assert(W.Toggle(), "workbench did not open")
local page = assert(W.GetPage("runner"), "Run page was not built")
assert(page:IsShown(), "Run page did not build a visible page frame")

-- Initial state.
assert(not page.exportResultButton:IsEnabled(), "Save started enabled on an empty result")
assert(page:GetResultMode() == "text" and page.resultPanel:IsShown()
        and not page.treeView.panel:IsShown(),
    "Run page did not start in text result mode")
assert(page.status:GetText() == L.READY, "Run page did not start READY")
assert(#page.historyButtons == 1 and page.historyButtons[1]:GetHeight() == 52,
    "history rows did not use the 52 px row height")
assert(page.historyButtons[1]:GetWidth() == 206, "history rows did not fit the 232 px rail")
assert(not page.rail.empty:IsShown(), "history empty label showed over a seeded entry")
assert(page.historyButtons[1].preview:GetText() == persistedCode,
    "history preview did not show the first code line")
assert(page.historyButtons[1].time:GetText() == os.date("%m-%d %H:%M", Env.now),
    "history timestamps did not use the %m-%d %H:%M format")

-- Run with a table return: tree + Save enabled, auto-selected tree mode.
page.inputPanel.editBox:SetText("return { nested = { value = 7 } }")
page.runButton:Click()
assert(page.treeView:HasTree(), "table result did not create a tree")
assert(page:GetResultMode() == "tree", "table result did not auto-select tree mode")
assert(page.exportResultButton:IsEnabled(), "run result export did not enable after execution")
assert(page.status:GetText() == L.COMPLETED, "successful run did not report COMPLETED")
assert(page:GetSelectedHistoryIndex() == 1, "new run was not selected as newest history")

-- Save (落盘) goes through the single export hook as run_result evidence.
local executeAfterRun = executeCount
page.exportResultButton:Click()
local order = ns.Stores.Exports.GetOrder()
local runEvidence = ns.Stores.Exports.Get(order[1])
assert(runEvidence and runEvidence.source.kind == "run_result"
        and runEvidence.source.title == L.RESULT
        and runEvidence.payload.content:find("nested", 1, true)
        and runEvidence.payload.content == page:GetResultText()
        and runEvidence.metadata.code == "return { nested = { value = 7 } }",
    "Run did not save its complete result and input as run_result evidence")
assert(executeCount == executeAfterRun, "Save unexpectedly executed code")

-- Run without return values: no tree, text mode.
page.inputPanel.editBox:SetText("print('no return value')")
page.runButton:Click()
assert(not page.treeView:HasTree(), "run without return values unexpectedly created a tree")
assert(page:GetResultMode() == "text", "no-return run did not fall back to text mode")

-- History restore: session live tree first, persisted stored tree second.
page.historyButtons[2]:Click()
assert(page.treeView:HasTree(), "current-session history did not restore its live tree")
assert(page.resultTreeTab:IsEnabled(), "restored history tree mode was not enabled")
assert(page.inputPanel.editBox:GetText() == "return { nested = { value = 7 } }",
    "history restore did not restore the code")
assert(page:GetResultMode() == "tree", "history restore did not select the tree for a tree entry")
assert(page.status:GetText() == L.COMPLETED, "history restore did not restore the completed status")

page.historyButtons[3]:Click()
assert(page.treeView:HasTree(), "persisted history did not restore its stored tree")
assert(page.inputPanel.editBox:GetText() == persistedCode,
    "persisted history did not restore its code")

-- History preview truncation at 22 characters + "...", first line only.
local longLine = string.rep("a", 30)
assert(ns.Stores.History.Add(longLine .. "\nsecond line", "r", true), "history entry was not stored")
page.RefreshHistory()
assert(page.historyButtons[1].preview:GetText() == longLine:sub(1, 22) .. "...",
    "history preview was not truncated at 22 characters")

-- Text / tree switching.
page.resultTextTab:Click()
assert(page.resultPanel:IsShown() and not page.treeView.panel:IsShown(),
    "result text mode did not activate")
page.resultTreeTab:Click()
assert(page.treeView.panel:IsShown() and not page.resultPanel:IsShown(),
    "result tree mode did not activate")

-- Select Result: text mode + select-all + focus, no clipboard API.
page.selectButton:Click()
assert(page.resultPanel:IsShown(), "Select Result did not switch to the text view")
assert(page.resultPanel.editBox.focused and page.resultPanel.editBox.highlighted,
    "Select Result did not select the result text")

-- Tab inserts 4 spaces.
page.inputPanel.editBox:SetText("")
Env.FireScript(page.inputPanel.editBox, "OnTabPressed")
assert(page.inputPanel.editBox:GetText() == "    ", "Tab did not insert 4 spaces")

-- Clear History: wipes history + tree cache + current result, keeps the input.
page.inputPanel.editBox:SetText("return 42")
page.runButton:Click()
local inputBeforeClearHistory = page.inputPanel.editBox:GetText()
assert(#ns.Stores.History.Get() > 0, "history was empty before the clear test")
page.clearHistoryButton:Click()
assert(#ns.Stores.History.Get() == 0 and page.rail.empty:IsShown(),
    "clear history did not empty the history list")
assert(page:GetResultText() == "" and page.resultPanel.editBox:GetText() == ""
        and not page.treeView:HasTree(),
    "clear history did not clear the current result")
assert(page.resultPanel:IsShown() and not page.treeView.panel:IsShown(),
    "clear history did not restore the empty text result view")
assert(not page.exportResultButton:IsEnabled() and page.status:GetText() == L.READY,
    "clear history did not reset result actions and status")
assert(page.inputPanel.editBox:GetText() == inputBeforeClearHistory,
    "clear history unexpectedly cleared the current Lua input")

-- Empty input: error as data (EMPTY_INPUT), no execution.
local executeBeforeEmpty = executeCount
page.inputPanel.editBox:SetText("   ")
page.runButton:Click()
assert(executeCount == executeBeforeEmpty, "empty input reached the executor")
assert(page:GetResultText() == L.EMPTY_INPUT and page.status:GetText() == L.FAILED,
    "empty input was not presented as EMPTY_INPUT error data")
local history = ns.Stores.History.Get()
assert(#history == 1 and history[1].code == "" and history[1].succeeded == false,
    "empty input did not record a failed history entry")
assert(page.historyButtons[1].preview:GetText() == L.EMPTY_INPUT,
    "empty history entry preview did not fall back to EMPTY_INPUT")

-- Combat: Run and Save are refused; no execution, no history entry, no export.
Env.SetInCombat(true)
local executeBeforeCombat = executeCount
local historyBeforeCombat = #ns.Stores.History.Get()
local exportBeforeCombat = #ns.Stores.Exports.GetOrder()
local statusBeforeCombat = page.status:GetText()
page.inputPanel.editBox:SetText("return 1")
page.runButton:Click()
assert(executeCount == executeBeforeCombat, "Run executed code during combat")
assert(#ns.Stores.History.Get() == historyBeforeCombat, "Run recorded history during combat")
assert(page.status:GetText() == statusBeforeCombat, "Run changed status during combat")
page.exportResultButton:Click()
assert(#ns.Stores.Exports.GetOrder() == exportBeforeCombat, "Save wrote an export during combat")
Env.SetInCombat(false)

-- Secret values in stored history previews fail open to a visible marker.
local secretMarker = Env.MakeSecret()
table.insert(ns.Stores.History.Get(), 1, {
    code = secretMarker,
    result = "r",
    succeeded = true,
    timestamp = Env.now,
})
page.RefreshHistory()
assert(page.historyButtons[1].preview:GetText() == L.TREE_SECRET,
    "secret history code was not masked in the preview")
page.historyButtons[1]:Click()
assert(page.inputPanel.editBox:GetText() == "", "secret history code leaked into the editor")

-- Combat shutdown hides the window and runs the page teardown.
page.inputPanel.editBox:SetFocus()
Env.SetInCombat(true)
ns.Safety.RunCombatShutdown()
assert(not LycheeToolkitWindow:IsShown(), "combat shutdown did not close the window")
assert(page.inputPanel.editBox.focused == false, "shutdown did not release the editor focus")
Env.SetInCombat(false)

print("Run page tests passed [" .. client .. "]")
