local root = assert(arg[1])
local secret = {}
issecretvalue = function(value) return rawequal(value, secret) end
local products = {
    { "Mainline", "12.1.0", 120100 }, { "Mists", "5.5.4", 50504 },
    { "Wrath", "3.80.2", 38002 }, { "Forever", "1.60.1", 16001 },
}
local outputs = {}
for _, profile in ipairs(products) do
    local ns, frames = {}, {}
    LycheeToolkitDB, SlashCmdList, SLASH_LYCHEETOOLKIT1 = nil, {}, nil
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
    local toc = assert(io.open(root .. "/Lychee Dev_" .. profile[1] .. ".toc"))
    for line in toc:lines() do
        line = line:gsub("\r", "")
        if line ~= "" and line:sub(1, 1) ~= "#" then
            assert(loadfile(root .. "/" .. line:gsub("\\", "/")))("Lychee Dev", ns)
        end
    end
    toc:close()
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
    fails(runner.Dispatch, "OP-bad-output", nil, "report_unsupported_value")
    fails(runner.Dispatch, "OP-bad-output", nil, "probe_already_dispatched")
    assert(runner.Load("OP-conflict", code))
    LycheeToolkitDB.reports["OP-conflict"] = false
    fails(runner.Dispatch, "OP-conflict", nil, "report_invalid_store")
    assert(probeCalls == 1, "corrupted report allowed code execution")
    LycheeToolkitDB.reports["OP-conflict"] = nil
    probeSecret = secret
    assert(runner.Load("OP-secret-error", "error(probeSecret)"))
    assert(runner.Dispatch("OP-secret-error"))
    local _, secretBody = ns.ReportStore.Read("OP-secret-error")
    assert(secretBody == '{"error":"probe_runtime_error","probeStatus":"failed"}')
    assert(runner.Load("OP-secret-result", "return probeSecret"))
    fails(runner.Dispatch, "OP-secret-result", nil, "report_secret_value")
    fails(runner.Dispatch, "OP-secret-result", nil, "probe_already_dispatched")
    assert(runner.Load("OP-old", code))
    local generation = ns.Session.Current().generation
    ns.Session.Release()
    assert(ns.Session.Bind(nonce).generation > generation)
    fails(runner.Dispatch, "OP-old", nil, "probe_not_loaded")
    fails(runner.Load, "OP-main", code, "probe_report_exists")
    LycheeToolkitDB.reports["OP-corrupt"] = false
    fails(runner.Load, "OP-corrupt", code, "report_invalid_store")
    LycheeToolkitDB.reports["OP-corrupt"] = nil
    assert(runner.Load("OP-change", "probeRecursive(); return 1"))
    probeRecursive = function() ns.Session.Release(); assert(ns.Session.Bind(nonce)) end
    fails(runner.Dispatch, "OP-change", nil, "probe_session_changed")
    assert(ns.ReportStore.Read("OP-change") == nil)
    local cleaned=0
    assert(runner.Load("OP-async", "local probe=...; assert(probe:Async(10)); assert(probe:OnCleanup(function() asyncCleaned=asyncCleaned+1 end)); probe:Log('waiting',42); asyncFinish=function() return probe:Finish({done=true}) end"))
    asyncCleaned=0
    local pending=assert(runner.Dispatch("OP-async"))
    assert(ns.ReportStore.Read("OP-async")==nil and pendingTimers[#pendingTimers].seconds==10)
    assert(asyncFinish() and asyncCleaned==1 and pendingTimers[#pendingTimers].cancelled)
    local _,asyncBody=ns.ReportStore.Read("OP-async")
    assert(asyncBody:find('"logs":["waiting  42"]',1,true) and asyncBody:find('"done":true',1,true))
    assert(runner.Load("OP-timeout", "local probe=...; assert(probe:Async(1)); assert(probe:OnCleanup(function() timeoutCleaned=true end))"))
    timeoutCleaned=false
    assert(runner.Dispatch("OP-timeout"))
    local timeoutTimer=pendingTimers[#pendingTimers]
    timeoutTimer.callback()
    local _,timeoutBody=ns.ReportStore.Read("OP-timeout")
    assert(timeoutCleaned and timeoutBody:find('"error":"probe_timeout"',1,true))
    -- Fresh generation, 16 definitions are retained, including executed ones.
    ns.Session.Release(); assert(ns.Session.Bind(nonce))
    for index = 1, 16 do assert(runner.Load("OP-count-" .. index, "return 1")) end
    fails(runner.Load, "OP-full", "return 1", "probe_queue_limit")
    ns.Session.Release(); assert(ns.Session.Bind(nonce))
    local large = "return 1" .. string.rep(" ", 256 * 1024 - 8)
    for index = 1, 4 do assert(runner.Load("OP-bytes-" .. index, large)) end
    fails(runner.Load, "OP-full", "return 1", "probe_queue_limit")
    assert(#frames == 1, "probe runner created runtime frames")
end
local ns = {}
assert(loadfile(root .. "/Bridge/CaptureWriter.lua"))("Lychee Dev", ns)
io.write(assert(ns.CaptureWriter.Encode(outputs)))
