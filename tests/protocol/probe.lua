local root = assert(arg[1])
assert(loadfile(arg[2]))()
local secret = {}
issecretvalue = function(value) return rawequal(value, secret) end
local products = {
    { "Mainline", "12.1.0", 120100 }, { "Mists", "5.5.4", 50504 },
    { "Wrath", "3.80.2", 38002 }, { "Forever", "1.60.1", 16001 },
}
local outputs = {}
for _, profile in ipairs(products) do
    local ns, frames = {}, {}
    LycheeToolkitDB, LycheeToolkitBridgeDB, SlashCmdList, SLASH_LYCHEETOOLKIT1 = nil, nil, {}, nil
    local pendingTimers={}
    C_Timer={NewTimer=function(seconds,callback)
        local timer={seconds=seconds,callback=callback,cancelled=false}
        function timer:Cancel() self.cancelled=true end
        pendingTimers[#pendingTimers+1]=timer
        return timer
    end}
    CreateFrame = function()
        local frame = {}
        function frame:RegisterEvent() end
        function frame:UnregisterAllEvents() end
        function frame:SetScript(_, callback) self.callback = callback end
        frames[#frames + 1] = frame
        return frame
    end
    GetBuildInfo = function() return profile[2], "12345", "date", profile[3] end
    UnitFullName = function() return "Paladin", "Realm" end
    UnitGUID = function() return "Player-1-123" end
    local toc = assert(io.open(root .. "/Lychee Dev.toc"))
    for line in toc:lines() do
        line = line:gsub("\r", "")
        if line ~= "" and line:sub(1, 1) ~= "#" then
            assert(loadfile(root .. "/" .. line:gsub("\\", "/")))("Lychee Dev", ns)
        end
    end
    toc:close()
    -- Legacy wire engine regression only; the production TOC selects SlotRuntime.
    ns.SlotRuntime=nil
    -- Renderer lifecycle has its own native-like UI fixture (t_activity.lua).
    -- Keep this protocol suite's no-executor-frames assertion independent.
    ns.ActivityView={Begin=function() end,Finish=function() end,Stop=function() end}

    frames[1].callback(frames[1], "ADDON_LOADED", "Lychee Dev")
    local function fails(method, a, b, expected)
        local result, reason = method(a, b)
        assert(result == nil and reason == expected, tostring(reason) .. " expected " .. expected)
    end
    local runner = ns.ProbeRunner
    fails(runner.Load, "OP-main", "return 42", "bridge_disabled")
    assert(ns.Controls.Handle("bridge on"))
    fails(runner.Load, "OP-main", "return 42", "session_unbound")
    local nonce = string.rep("a", 32)
    assert(ns.Session.Bind(nonce))
    fails(runner.Load, secret, "return 42", "probe_invalid_request")
    fails(runner.Load, "OP-invalid", secret, "probe_invalid_code")
    fails(runner.Load, "OP-invalid", "", "probe_invalid_code")
    fails(runner.Load, "OP-invalid", string.char(27) .. "Lua", "probe_invalid_code")
    fails(runner.Load, "OP-invalid", string.rep(" ", 256 * 1024 + 1), "probe_invalid_code")
    fails(runner.Load, "OP-invalid", "return )", "probe_syntax_error")
    assert(ns.Session.Current().sequence == 0)
    probeCalls = 0
    local code = "probeCalls = probeCalls + 1; return {answer = 42}"
    local loaded = assert(runner.Load("OP-main", code))
    assert(probeCalls == 0, "load executed code")
    assert(ns.Session.Bind(nonce), "idempotent bind failed")
    fails(runner.Load, "OP-main", code, "probe_request_exists")
    local report = assert(runner.Dispatch("OP-main"))
    assert(probeCalls == 1)
    fails(runner.Dispatch, "OP-main", nil, "probe_already_dispatched")
    local receipt, body = ns.ReportStore.Read("OP-main")
    assert(receipt == report)
    outputs[#outputs + 1] = { loaded = loaded, receipt = receipt, body = body, code = code }
    assert(runner.Load("OP-error", 'error("expected failure")'))
    assert(runner.Dispatch("OP-error"))
    local _, errorBody = ns.ReportStore.Read("OP-error")
    assert(string.find(errorBody, '"probeStatus":"failed"', 1, true))
    probeRecursive = function()
        fails(runner.Dispatch, "OP-recursive", nil, "probe_already_dispatched")
    end
    assert(runner.Load("OP-recursive", "probeRecursive(); return 1"))
    assert(runner.Dispatch("OP-recursive"))
    assert(runner.Load("OP-bad-output", "return function() end"))
    local badOutput=assert(runner.Dispatch("OP-bad-output"))
    assert(badOutput:find('"kind":"report_error"',1,true) and
        badOutput:find('"errorCode":"report_unsupported_value"',1,true))
    assert(ns.ReportStore.Read("OP-bad-output")==nil)
    fails(runner.Dispatch, "OP-bad-output", nil, "probe_already_dispatched")
    -- An asynchronous encoding failure has only a minimal optical marker.
    -- Its producer must refresh the same terminal marker after a receiver
    -- accepted query temporarily takes over the display.
    local previousShow = ns.ReceiptView.Show
    local failureRefresh
    ns.ReceiptView.Show = function(receipt, readiness, refresh)
        failureRefresh = refresh
        return true
    end
    assert(runner.Load("OP-async-output-error",
        "local probe=...; assert(probe:Async(10)); asyncOutputError=function() return probe:Finish(function() end) end"))
    assert(runner.Dispatch("OP-async-output-error"))
    local asyncFailureReceipt = assert(asyncOutputError())
    assert(asyncFailureReceipt:find('"kind":"report_error"',1,true))
    assert(failureRefresh and failureRefresh() == asyncFailureReceipt,
        "minimal report error did not refresh after receiver query")
    assert(ns.ReportStore.Read("OP-async-output-error") == nil,
        "minimal failure was mistaken for a stored full report")
    ns.ReceiptView.Show = previousShow
    assert(runner.Load("OP-conflict", code))
    LycheeToolkitBridgeDB.reports["OP-conflict"] = false
    fails(runner.Dispatch, "OP-conflict", nil, "report_invalid_store")
    assert(probeCalls == 1, "corrupted report allowed code execution")
    LycheeToolkitBridgeDB.reports["OP-conflict"] = nil
    probeSecret = secret
    assert(runner.Load("OP-secret-error", "error(probeSecret)"))
    assert(runner.Dispatch("OP-secret-error"))
    local _, secretBody = ns.ReportStore.Read("OP-secret-error")
    assert(secretBody == '{"acceptedBudgetSeconds":120,"error":"probe_runtime_error","probeStatus":"failed"}')
    assert(runner.Load("OP-secret-result", "return probeSecret"))
    local secretError=assert(runner.Dispatch("OP-secret-result"))
    assert(secretError:find('"kind":"report_error"',1,true) and
        secretError:find('"errorCode":"report_secret_value"',1,true))
    fails(runner.Dispatch, "OP-secret-result", nil, "probe_already_dispatched")
    assert(runner.Load("OP-big-result", "return string.rep('x', 524289)"))
    local bigError=assert(runner.Dispatch("OP-big-result"))
    assert(bigError:find('"kind":"report_error"',1,true) and
        bigError:find('"errorCode":"report_byte_limit"',1,true))
    assert(ns.ReportStore.Read("OP-big-result")==nil)
    assert(runner.Load("OP-old", code))
    local generation = ns.Session.Current().generation
    ns.Session.Release()
    assert(ns.Session.Bind(nonce).generation > generation)
    fails(runner.Dispatch, "OP-old", nil, "probe_not_loaded")
    fails(runner.Load, "OP-main", code, "probe_report_exists")
    LycheeToolkitBridgeDB.reports["OP-corrupt"] = false
    fails(runner.Load, "OP-corrupt", code, "report_invalid_store")
    LycheeToolkitBridgeDB.reports["OP-corrupt"] = nil
    local cleaned=0
    local originalView, visibleReceipt = ns.ReceiptView, "old-handshake"
    ns.ReceiptView = {Hide=function() visibleReceipt=nil end,
        Show=function(value) visibleReceipt=value;return true end}
    assertAsyncScreenClear=function() assert(visibleReceipt==nil,"handshake visible at business entry") end
    assert(runner.Load("OP-async", "local probe=...; assertAsyncScreenClear(); assert(probe:Async(10)); assert(probe:OnCleanup(function() asyncCleaned=asyncCleaned+1 end)); probe:Log('waiting',42); asyncFinish=function() return probe:Finish({done=true}) end"))
    asyncCleaned=0
    local pending=assert(ns.Controls.Handle("bridge run OP-async"))
    assert(visibleReceipt==nil,"async execution redisplayed a handshake over the scene")
    assert(ns.ReportStore.Read("OP-async")==nil and pendingTimers[#pendingTimers].seconds==10)
    assert(asyncFinish() and asyncCleaned==1 and pendingTimers[#pendingTimers].cancelled)
    assert(visibleReceipt and visibleReceipt:find('"kind":"reported"',1,true),"terminal report was not displayed")
    ns.ReceiptView=originalView
    local _,asyncBody=ns.ReportStore.Read("OP-async")
    assert(asyncBody:find('"logs":["waiting  42"]',1,true) and asyncBody:find('"done":true',1,true))
    assert(runner.Load("OP-timeout", "local probe=...; assert(probe:Async(1)); assert(probe:OnCleanup(function() timeoutCleaned=true end))"))
    timeoutCleaned=false
    assert(runner.Dispatch("OP-timeout"))
    local timeoutTimer=pendingTimers[#pendingTimers]
    timeoutTimer.callback()
    local _,timeoutBody=ns.ReportStore.Read("OP-timeout")
    assert(timeoutCleaned and timeoutBody:find('"error":"probe_timeout"',1,true))
    assert(runner.Load("OP-budget", "local probe=...; local ok,reason=probe:Async(6); assert(ok==nil and reason=='probe_async_unavailable'); return 1",nil,5))
    assert(runner.Dispatch("OP-budget"))
    local _,budgetBody=ns.ReportStore.Read("OP-budget")
    assert(budgetBody:find('"acceptedBudgetSeconds":5',1,true))
    -- Fresh generation, 16 definitions are retained, including executed ones.
    ns.Session.Release(); assert(ns.Session.Bind(nonce))
    for index = 1, 16 do assert(runner.Load("OP-count-" .. index, "return 1")) end
    fails(runner.Load, "OP-full", "return 1", "probe_queue_limit")
    ns.Session.Release(); assert(ns.Session.Bind(nonce))
    local large = "return 1" .. string.rep(" ", 256 * 1024 - 8)
    for index = 1, 4 do assert(runner.Load("OP-bytes-" .. index, large)) end
    fails(runner.Load, "OP-full", "return 1", "probe_queue_limit")
    ns.Session.Release(); assert(ns.Session.Bind(nonce))
    assert(runner.Load("OP-early-finish", 'local p=...; assert(p:Async(10)); assert(p:Finish({ok=true})); error("after_finish_failure")'))
    assert(runner.Dispatch("OP-early-finish"))
    local _,earlyBody=ns.ReportStore.Read("OP-early-finish")
    assert(earlyBody:find('"probeStatus":"failed"',1,true) and earlyBody:find('after_finish_failure',1,true),earlyBody)
    local oldClock=GetTime
    GetTime=function() return 100 end
    auditAdvanceClock=function() GetTime=function() return 103 end end
    assert(runner.Load("OP-late-async", 'local p=...; auditAdvanceClock(); assert(p:Async(10))'))
    assert(runner.Dispatch("OP-late-async"))
    assert(pendingTimers[#pendingTimers].seconds==7,"Async extended the original deadline")
    pendingTimers[#pendingTimers].callback()
    GetTime=oldClock
    auditCallbackEffects=0
    assert(runner.Load("OP-callback", 'local p=...; assert(p:Async(10)); auditCallback=assert(p:Callback(function() auditCallbackEffects=auditCallbackEffects+1; assert(p:Finish(true)); error("callback_after_finish") end))'))
    assert(runner.Dispatch("OP-callback"))
    assert(runner.Busy())
    assert(ns.ProbeQueue.Reset(string.rep("c",32))==nil,"Reset erased running work")
    assert(auditCallback())
    local _,callbackBody=ns.ReportStore.Read("OP-callback")
    assert(callbackBody:find('"probeStatus":"failed"',1,true) and callbackBody:find('callback_after_finish',1,true))
    assert(auditCallback()==nil and auditCallbackEffects==1,"late callback entered user code")
    assert(not runner.Busy())
    auditCleanupCount=0
    assert(runner.Load("OP-cleanup-failure", 'local p=...; assert(p:OnCleanup(function() auditCleanupCount=auditCleanupCount+1 end)); assert(p:OnCleanup(function() error("cleanup_failed") end)); return true'))
    assert(runner.Dispatch("OP-cleanup-failure"))
    local _,cleanupBody=ns.ReportStore.Read("OP-cleanup-failure")
    assert(auditCleanupCount==1 and cleanupBody:find('"resources":',1,true) and cleanupBody:find('cleanup_failed',1,true),cleanupBody)
    assert(runner.Busy() and not runner.ResourcesReleased("OP-cleanup-failure"))
    ns.Session.Release(); assert(ns.Session.Bind(nonce))
    fails(runner.Load,"OP-after-cleanup","return true","probe_runtime_busy")
    -- Independent runtime: changing input sessions during execution retains
    -- unresolved ownership. This is the existing session-change regression,
    -- now also proving a reconnect cannot erase the old executor.
    local isolated={Release=ns.Release,Startup=ns.Startup,Platform=ns.Platform,Compat=ns.Compat}
    for _,name in ipairs({"Core/Persistence.lua","Bridge/CaptureWriter.lua","Bridge/Session.lua","Bridge/ReportStore.lua","Bridge/ProbeRunner.lua"}) do
        assert(loadfile(root.."/"..name))("Lychee Dev",isolated)
    end
    LycheeToolkitDB, LycheeToolkitBridgeDB = nil, nil
    assert(isolated.Persistence.Load())
    LycheeToolkitDB.options.bridgeEnabled=true
    assert(isolated.Session.Bind(nonce))
    probeRecursive=function() isolated.Session.Release(); assert(isolated.Session.Bind(nonce)) end
    assert(isolated.ProbeRunner.Load("OP-change","probeRecursive(); return 1"))
    fails(isolated.ProbeRunner.Dispatch,"OP-change",nil,"probe_session_changed")
    assert(isolated.ReportStore.Read("OP-change")==nil and isolated.ProbeRunner.Busy())
    fails(isolated.ProbeRunner.Load,"OP-after-change","return true","probe_runtime_busy")
    assert(#frames == 1, "probe runner created runtime frames")
end
local ns = {}
assert(loadfile(root .. "/Bridge/CaptureWriter.lua"))("Lychee Dev", ns)
io.write(assert(ns.CaptureWriter.Encode(outputs)))
