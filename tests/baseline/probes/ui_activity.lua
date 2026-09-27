local probe=...
assert(probe:Async(75))
local ns=LycheeDevInternal
assert(ns.ActivityView, "old_runtime")
local timers,pages={},{}
local function check()
    assert(not ns.Receiver.IsActive(),"receiver_blocks_probe")
    assert(ns.ReceiptView.Current()==nil,"probe_receipt_overlay")
    assert(ns.ActivityView.Current()=="probe","probe_activity_lost")
end
local function later(seconds,callback)
    timers[#timers+1]=C_Timer.NewTimer(seconds,assert(probe:Callback(function() check();callback() end)))
end
assert(probe:OnCleanup(function()
    for _,timer in ipairs(timers) do timer:Cancel() end
    ns.Workbench.Close()
end))
check()
assert(ns.Workbench.ShowPage("events"))
pages[#pages+1]="events-empty"
later(8,function()
    local page=ns.Workbench.GetPage("events")
    page.inputPanel.editBox:SetFocus()
    page.inputPanel.editBox:SetText("PLAYER_ENTERING_WORLD")
    later(0.5,function()
    page.inputPanel.editBox:GetScript("OnEnterPressed")(page.inputPanel.editBox)
    page.inputPanel.editBox:ClearFocus()
    assert(ns.ActivityView.Current()=="probe","normal_text_focus_interrupted_activity")
    assert(page.selectedPanel:GetAlpha()==1 and page.monitorButton:IsEnabled(),"event_selection_missing")
    pages[#pages+1]="events-selected"
    end)
end)
later(16,function() assert(ns.Workbench.ShowPage("diagnostics"));pages[#pages+1]="diagnostics" end)
later(24,function()
    assert(ns.Workbench.ShowPage("automation"))
    local page=ns.Workbench.GetPage("automation")
    local _,id=ns.ActivityView.Current()
    page.SelectRecord(id)
    assert(page.statusValue:GetText()==ns.L.AUTO_STATUS_RUNNING,"running_status_incorrect")
    assert(not page.executeButton:IsEnabled(),"running_execute_enabled")
    pages[#pages+1]="automation-result"
end)
later(32,function()
    local page=ns.Workbench.GetPage("automation")
    page.detailsButton:GetScript("OnClick")(page.detailsButton)
    assert(page.metadata:IsShown() and not page.reportArea:IsShown())
    pages[#pages+1]="automation-details"
end)
later(40,function()
    assert(ns.Workbench.ShowSettings())
    local settings=ns.Workbench.GetSettingsPage()
    assert(not settings.saveBindings and not settings.resetBindings)
    for _,action in ipairs({"wake","submit","close"}) do
        assert(not settings.bindings[action].edit)
        assert(not settings.bindings[action].button:IsMouseEnabled())
    end
    pages[#pages+1]="fixed-shortcuts"
end)
later(48,function() ns.Workbench.Close();pages[#pages+1]="probe-indicator-only" end)
later(56,function() probe:Finish({passed=true,pages=pages,probeActivity=true,inputReleased=true,fixedShortcuts=true}) end)
