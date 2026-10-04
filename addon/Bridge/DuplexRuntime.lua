local _, ns = ...

-- Opt-in owner for the Lychee Dev mailbox protocol v1. Its arena and controller
-- stay strongly reachable from this closure; the public object carries cells.
local owner, arena, engine, ticker, generation, retiredArena, repairPending, arenaRoots, retiredRoots = nil, nil, nil, nil, 0, nil, false, nil, nil
local privatePages, projectedPages = {}, nil
local CALIBRATION_EXPECTED={0,1,4294967295,0.125,-13.5,7654321}
local ROW_KEYS={"command","stop"}
local function restricted(v)return issecretvalue and issecretvalue(v)end
local function plainTable(v)return not restricted(v) and type(v)=="table" and getmetatable(v)==nil end
local function freshToken(seed)
    local clock=ns.Compat.MonotonicSeconds() or 0
    local words={string.format("%08x",math.floor(clock*1000)%4294967296)}
    for i=1,3 do words[#words+1]=string.format("%08x",math.random(0,65535)*65536+math.random(0,65535)) end
    words[#words+1]=string.format("%08x",math.random(0,65535)*65536+math.random(0,65535))
    local digest=ns.SHA256.Digest(table.concat(words)..":"..tostring(seed or "")..":"..tostring({}))
    return ns.SHA256.Hex(digest):sub(1,32)
end
local function currentActor()
    local actor=ns.Platform.ObserveActor()
    if not actor or restricted(actor.guid) or restricted(actor.character) or restricted(actor.realm) then return nil end
    return actor
end
local function enabledByOwner()
    local state=ns.Persistence.Current()
    return state and state.options and state.options.bridgeEnabled==true
end
local function publicMailbox(identity, cells)
    local pages={};for i,v in pairs(privatePages) do pages[i]=v end;projectedPages=pages
    local sendbox={status="",receipts="",resultPages=pages}
    local inbox={calibration=cells.calibration,command=cells.command,stop=cells.stop}
    return {schema="lycheedev.mailbox.v1",layoutId="single-command-row-v1",release=ns.Release,runtime=identity.runtime,
        arenaGeneration=identity.arenaGeneration,inbox=inbox,sendbox=sendbox}
end
local function ensureSendbox()
    if not plainTable(owner) then return nil end
    local sendbox=rawget(owner,"sendbox")
    if not plainTable(sendbox) then sendbox={status="",receipts="",resultPages={}};rawset(owner,"sendbox",sendbox);projectedPages=nil end
    local pages=rawget(sendbox,"resultPages")
    if not plainTable(pages) or pages~=projectedPages then
        pages={};for i,v in pairs(privatePages) do pages[i]=v end
        rawset(sendbox,"resultPages",pages);projectedPages=pages
    else
        local damaged=false
        for i,v in pairs(privatePages) do if rawget(pages,i)~=v then damaged=true;break end end
        if damaged then
            pages={};for i,v in pairs(privatePages) do pages[i]=v end
            rawset(sendbox,"resultPages",pages);projectedPages=pages
        end
    end
    return sendbox
end
local function publish(kind,wire)
    if not owner or type(wire)~="string" then return nil end
    if kind=="results" then return nil end
    local sendbox=ensureSendbox();if not sendbox then return nil end
    rawset(sendbox,kind=="receipts" and "receipts" or "status",wire)
    -- Changed only invokes the handler installed by the visible workbench.
    -- Publications occur at actual transitions or the bounded heartbeat,
    -- never on every idle transport poll.
    if ns.AutomationView and type(ns.AutomationView.Changed)=="function" then pcall(ns.AutomationView.Changed) end
    return true
end
local function makeOwner()
    local build,reason=ns.Platform.ObserveBuild();if not build then return nil,reason end
    local actor;actor,reason=currentActor();if not actor then return nil,"duplex_actor_unavailable" end
    generation=generation+1
    local runtime=freshToken(generation);local arenaGeneration=freshToken("arena:"..generation)
    local identity={runtime=runtime,arenaGeneration=arenaGeneration,release=ns.Release,
        actorGUID=actor.guid,character=actor.character,realm=actor.realm,build=build.build,
        product=build.product,version=build.build,
        actorBindingId=ns.SHA256.Hex(ns.SHA256.Digest("LYCMBX/actor/v1\0"..actor.guid.."\0"..build.build.."\0"..build.product)):sub(1,32)}
    local cells,roots=ns.DuplexProtocol.NewArena(arenaGeneration);if not cells then return nil,roots end
    local box=publicMailbox(identity,cells)
    local challenge=function()
        local raw=ns.SHA256.Digest(runtime..arenaGeneration..tostring(ns.Compat.MonotonicSeconds() or 0)..tostring(math.random()))
        return raw:sub(1,16)
    end
    local controller
    controller,reason=ns.DuplexProtocol.Create({identity=identity,publish=publish,
        compile=function(source)return loadstring(source,"=LycheeDuplexProbe")end,
        execute=ns.ProbeExecution.Run,encode=ns.CaptureWriter.Encode,
        clock=ns.Compat.MonotonicSeconds,workClockMillis=ns.Compat.ProfileMilliseconds,challenge=challenge,
        actor=function()local a=currentActor();if a and a.guid==identity.actorGUID then return a end end,
        reload=function()return type(ReloadUI)=="function" end,
        clearPages=function()
            for i in pairs(privatePages) do privatePages[i]=nil end
            local box=ensureSendbox();if box then rawset(box,"resultPages",{});projectedPages=rawget(box,"resultPages") end
        end,
        started=function(id)if ns.ActivityView then ns.ActivityView.RunStarted(id) end end,
        finished=function(id)if ns.ActivityView then ns.ActivityView.RunFinished(id) end end,
        publishPage=function(index,wire)
            privatePages[index]=wire
            local box=ensureSendbox();if box then local pages=rawget(box,"resultPages");if plainTable(pages) then rawset(pages,index,wire) end end
        end})
    if not controller then return nil,reason end
    controller.BindIdentity(identity)
    owner,arena,engine,arenaRoots=box,cells,controller,roots
    ns.Mailbox=box
    engine.Enable();engine.PublishStatus()
    return true
end
local heartbeatElapsed=0
local function repairArena()
    if not engine.HasOwner() then
        engine.Quarantine("duplex_repair_unavailable")
        return nil,"duplex_repair_unavailable"
    end
    if retiredArena or retiredRoots then
        engine.Quarantine("duplex_arena_damaged_during_repair")
        return nil,"duplex_arena_damaged_during_repair"
    end
    local state=engine.Snapshot()
    local safeIdle=not state.request and not state.terminal and not state.closing
        and state.phase=="ready_unbound"
    if not safeIdle then
        engine.Quarantine("duplex_arena_state_unknown")
        return nil,"duplex_arena_state_unknown"
    end
    local newGeneration=freshToken("repair:"..tostring(generation+1))
    local cells,roots=ns.DuplexProtocol.NewArena(newGeneration)
    if not cells then engine.Quarantine(roots or "duplex_arena_rebuild_failed");return nil,roots end
    local challengeRaw=ns.SHA256.Digest(newGeneration..tostring(math.random())..tostring(ns.Compat.MonotonicSeconds() or 0))
    challengeRaw=challengeRaw:sub(1,16)
    local ok,reason;ok,reason=engine.BeginRepair(newGeneration,challengeRaw)
    if not ok then engine.Quarantine(reason or "duplex_arena_rebuild_failed");return nil,reason end
    retiredArena=arena;retiredRoots=arenaRoots;repairPending=true;generation=generation+1
    arena,arenaRoots=cells,roots
    local identity=engine.Snapshot().identity
    owner=publicMailbox(identity,cells);ns.Mailbox=owner
    engine.PublishStatus()
    return true
end
local function topologyValid()
    if not plainTable(owner) or not plainTable(arena) or not plainTable(arenaRoots) then return false end
    local inbox=rawget(owner,"inbox")
    if not plainTable(inbox) then return false end
    local calibration=rawget(inbox,"calibration")
    if not plainTable(calibration) or calibration~=rawget(arena,"calibration") or calibration~=rawget(arenaRoots,"calibration") or #calibration~=6 then return false end
    for i=1,6 do local value=rawget(calibration,i);if restricted(value) or value~=CALIBRATION_EXPECTED[i] then return false end end
    for _,key in ipairs(ROW_KEYS) do
        local row=rawget(inbox,key)
        local expected=key=="command" and 80+ns.DuplexProtocol.FrameBytes/4 or 336
        if not plainTable(row) or row~=rawget(arena,key) or row~=rawget(arenaRoots,key) or #row~=expected
            or table.isfrozen(row)~=true then return false end
    end
    return true
end
local pollElapsed=0
local function tick(elapsed)
    if not owner or not arena or not engine or not enabledByOwner() then return end
    heartbeatElapsed=heartbeatElapsed+(elapsed or 0)
    pollElapsed=pollElapsed+(elapsed or 0)
    if pollElapsed<0.05 and not engine.HasPendingValidation() then return end
    pollElapsed=pollElapsed%0.05
    if engine.TakeReload and engine.TakeReload() then
        local ok=pcall(ReloadUI)
        if not ok then engine.Quarantine("duplex_reload_failed") end
        return
    end
    if ns.Mailbox~=owner then ns.Mailbox=owner;return end
    local quarantined=engine.RuntimeState()
    if quarantined then return end
    -- Validate all public roots and row identities before indexed access. A
    -- damaged topology is repaired from private references without reading it.
    if not topologyValid() then
        repairArena();return
    end
    local enabled=engine.Enable();if not enabled then return end
    engine.Poll(arena,8)
    local _,_,repaired=engine.RuntimeState()
    -- A single whole-row OS write is not atomic. While it is in progress,
    -- content and padding checks can reject a mixed snapshot. The same row is
    -- retried after the write completes; only damaged topology starts repair.
    if repairPending and repaired then
        engine.ReleaseRetiredArena();retiredArena=nil;retiredRoots=nil;repairPending=false
    end
    if heartbeatElapsed>=1 then heartbeatElapsed=heartbeatElapsed-1;engine.Heartbeat() end
end
local function createTicker()
    if ticker then return true end
    local ok,frame=pcall(CreateFrame,"Frame")
    if not ok or not frame then return nil,"duplex_frame_unavailable" end
    ticker=frame
    frame:SetScript("OnUpdate",function(_,elapsed)tick(elapsed)end)
    return true
end
ns.DuplexRuntime={
    Start=function()
        if not enabledByOwner() then return true,"duplex_disabled" end
        if owner then if ns.Mailbox~=owner then ns.Mailbox=owner end;return true end
        local ok,reason=makeOwner();if not ok then return nil,reason end
        return true
    end,
    Enable=function()
        if not enabledByOwner() then return nil,"duplex_disabled" end
        local ok,reason=ns.DuplexRuntime.Start();if not ok then return nil,reason end
        ok,reason=createTicker();if not ok then return nil,reason end
        local enabled,enableReason=engine.Enable();if not enabled then return nil,enableReason end;return true
    end,
    Disable=function()
        if engine then local ok,reason=engine.Disable();if not ok then return nil,reason end end
        if ticker then ticker:SetScript("OnUpdate",nil);ticker:Hide();ticker=nil end
        return true
    end,
    Snapshot=function()
        return {enabled=ticker~=nil,runtime=owner and owner.runtime or nil,
            arenaGeneration=owner and owner.arenaGeneration or nil,protocol=engine and engine.Snapshot() or nil}
    end,
}
