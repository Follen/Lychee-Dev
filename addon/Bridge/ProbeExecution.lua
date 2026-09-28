local _, ns = ...

-- Runtime-local execution, independent of transport, display and persistence.
-- Async callbacks are bounded; Lua pcall is not a synchronous CPU sandbox.
ns.ProbeExecution={Run=function(fn,budget,done)
    local live,async,depth=true,false,0
    local started=ns.Compat.MonotonicSeconds()
    local timer,pending
    local cleanups,logs={},{}
    local callbacks,logBytes=0,0
    local protection,protectionUsed,guardFrame
    local guards,events={},{}
    local api={}
    local function finish()
        if not live or not pending or depth>0 then return end
        live=false
        local outcome=pending;pending=nil
        if timer then pcall(timer.Cancel,timer);timer=nil end
        local clean=true
        -- Release our lease before user cleanup, even if that cleanup throws.
        if protection then
            if not pcall(ns.InputProtection.Release,protection) then clean=false end
            protection=nil
        end
        if guardFrame then
            if not pcall(function()guardFrame:UnregisterAllEvents();guardFrame:SetScript("OnEvent",nil)end) then clean=false end
        end
        for i=#cleanups,1,-1 do if not pcall(cleanups[i]) then clean=false end end
        cleanups={}
        done(outcome.ok,outcome.value,{resourcesReleased=clean,logs=logs})
    end
    local function settle(ok,value)
        if not live then return nil,"probe_not_running" end
        if not pending or not ok then pending={ok=ok,value=value} end
        finish();return true
    end
    local function checkGuards()
        for _,guard in ipairs(guards) do
            local ok,value=pcall(guard.check)
            if not ok or (issecretvalue and issecretvalue(value)) or value~=true then
                settle(false,guard.reason);return nil,guard.reason
            end
        end
        return true
    end
    function api:Guard(check, watchedEvents, reason)
        if not live or pending or type(check)~="function" or #guards>=8
            or (issecretvalue and issecretvalue(reason)) or type(reason)~="string" or #reason==0 or #reason>128
            or type(watchedEvents)~="table" or getmetatable(watchedEvents) then return nil,"probe_invalid_guard" end
        local newEvents={}
        for i,event in ipairs(watchedEvents) do
            if i>8 or (issecretvalue and issecretvalue(event)) or type(event)~="string"
                or #event>80 or not event:match("^[A-Z][A-Z0-9_]+$") then return nil,"probe_invalid_guard_event" end
            newEvents[event]=true
        end
        local count=0
        for _ in pairs(events) do count=count+1 end
        for event in pairs(newEvents) do if not events[event] then count=count+1 end end
        if count>8 then return nil,"probe_guard_event_limit" end
        guards[#guards+1]={check=check,reason=reason}
        local ok,failure=checkGuards();if not ok then return nil,failure end
        if next(newEvents) then
            local registered=pcall(function()
                if not guardFrame then
                    guardFrame=CreateFrame("Frame")
                    guardFrame:SetScript("OnEvent",function()if live and not pending then checkGuards() end end)
                end
                for event in pairs(newEvents) do guardFrame:RegisterEvent(event);events[event]=true end
            end)
            if not registered then settle(false,"probe_guard_event_unavailable");return nil,"probe_guard_event_unavailable" end
        end
        return true
    end
    function api:ProtectInput(seconds)
        if not live or pending or protectionUsed then return nil,"probe_input_protection_used" end
        if (issecretvalue and issecretvalue(seconds)) or type(seconds)~="number" or seconds~=seconds
            or seconds<=0 or seconds==math.huge or not started then return nil,"probe_invalid_protection_budget" end
        local remaining=budget-(ns.Compat.MonotonicSeconds()-started)
        if seconds>remaining then return nil,"probe_budget_exhausted" end
        local ok,reason=checkGuards();if not ok then return nil,reason end
        if not ns.InputProtection then return nil,"probe_input_protection_unavailable" end
        protection,reason=ns.InputProtection.Acquire(seconds,function(failure)
            protection=nil;settle(false,failure)
        end)
        if not protection then return nil,reason end
        protectionUsed=true;return true
    end
    function api:ReleaseInput()
        if not protection then return false end
        local token=protection;protection=nil
        return ns.InputProtection.Release(token)
    end
    function api:IsInputProtected()return protection~=nil end
    function api:Async(seconds)
        if not live or async or not started or type(seconds)~="number" or seconds<1 or seconds>budget then return nil,"probe_invalid_async_budget" end
        if type(C_Timer)~="table" or type(C_Timer.NewTimer)~="function" then return nil,"probe_async_unavailable" end
        local remaining=seconds-(ns.Compat.MonotonicSeconds()-started)
        if remaining<=0 then return nil,"probe_budget_exhausted" end
        timer=C_Timer.NewTimer(remaining,function()settle(false,"probe_timeout")end)
        if not timer then return nil,"probe_async_unavailable" end
        async=true;return true
    end
    function api:Finish(value)
        if not live then return nil,"probe_not_running" end
        local valid,reason=checkGuards();if not valid then return nil,reason end
        return settle(true,value)
    end
    function api:Fail(value)
        if (issecretvalue and issecretvalue(value)) or type(value)~="string" or #value>4096 then value="probe_runtime_error" end
        return settle(false,value)
    end
    function api:Callback(callback)
        if not live or type(callback)~="function" or callbacks>=16 then return nil,"probe_callback_limit" end
        callbacks=callbacks+1
        return function(...)
            if not live or pending then return nil,"probe_callback_inactive" end
            local valid,reason=checkGuards();if not valid then return nil,reason end
            depth=depth+1;local ok,result=pcall(callback,...);depth=depth-1
            if not ok then api:Fail(result) else finish() end
            return ok,result
        end
    end
    function api:OnCleanup(callback)
        if not live or type(callback)~="function" or #cleanups>=16 then return nil,"probe_cleanup_limit" end
        cleanups[#cleanups+1]=callback;return true
    end
    function api:Log(...)
        if not live or #logs>=100 or logBytes>=32768 then return nil,"probe_log_limit" end
        local parts={}
        for i=1,select("#",...) do
            local value=select(i,...)
            if issecretvalue and issecretvalue(value) then parts[i]="<secret>"
            elseif type(value)=="string" or type(value)=="number" or type(value)=="boolean" then parts[i]=tostring(value)
            else parts[i]="<unsupported>" end
        end
        local line=table.concat(parts,"  "):sub(1,32768-logBytes)
        logs[#logs+1]=line;logBytes=logBytes+#line;return true
    end
    function api:IsCancelled()return not live end
    depth=1;local ok,result=pcall(fn,api);depth=0
    if not ok then api:Fail(result)
    elseif pending then finish()
    elseif not async then
        local valid=checkGuards();if valid then settle(true,result) end
    end
    return api
end}
