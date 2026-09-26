local root=assert(arg[1])
assert(loadfile(arg[2]))()
local secret={}
issecretvalue=function(value) return rawequal(value,secret) end
local profiles={
    {version="12.1.0",interface=120100,product="retail"},
    {version="5.5.4",interface=50504,product="classic"},
    {version="3.80.2",interface=38002,product="titan"},
    {version="1.60.1",interface=16001,product="forever"},
}
local outputs={}
for _,profile in ipairs(profiles) do
    for _,mode in ipairs({"success","world-first","loading-restart","world-exit","login","initial","secret","actor","epoch","queue","code","body","replace","edit","off","disabled","future","submission","cleanup","cleanup-queue","cleanup-report","prepare","prepare-missing","prepare-nonce","prepare-actor","prepare-login"}) do
        local cleanup=mode:sub(1,7)=="cleanup"
        local preparing=mode:sub(1,7)=="prepare"
        local actor={character="Paladin",realm="Realm",guid="Player-1-123"}
        local frames,shown,reloads={},nil,0
        local nonce,reloadNonce=string.rep("a",32),string.rep("b",32)
        local code="reentryExecutions=(reentryExecutions or 0)+1; return 42"
        reentryExecutions=0
        LycheeToolkitDB=nil
        GetBuildInfo=function() return profile.version,"12345","date",profile.interface end
        UnitFullName=function() return actor.character,actor.realm end
        UnitGUID=function() return actor.guid end
        IsLoggedIn=function() return true end
        InCombatLockdown=function() return false end
        GetCurrentKeyBoardFocus=function() return nil end
        C_UI={Reload=function()
            reloads=reloads+1
            assert(LycheeToolkitDB.reentry and LycheeToolkitDB.reentry.requestId=="Reload-A")
            if mode=="submission" then error("synthetic submission failure") end
        end}
        CreateFrame=function(kind)
            assert(kind=="Frame")
            local frame={events={}}
            function frame:RegisterEvent(event) self.events[event]=true end
            function frame:UnregisterAllEvents() self.events={} end
            function frame:SetScript(event,callback) assert(event=="OnEvent"); self.callback=callback end
            frames[#frames+1]=frame
            return frame
        end
        local function loadRuntime(changedQueue)
            SlashCmdList,SLASH_LYCHEETOOLKIT1={},nil
            local ns={}
            local toc=assert(io.open(root.."/Lychee Dev.toc","r"))
            for line in toc:lines() do
                line=line:gsub("\r",""):gsub("\\","/")
                if line~="" and line:sub(1,1)~="#" then
                    if line=="Bridge/ProbeQueue.lua" then
                        ns.ProbeDefinitions={schema="lycheedev.queue.v1",entries={ ["Reload-A"]={
                            release="2.0.3",product=profile.product,build=profile.version..".12345",
                            character="Paladin",realm="Realm",guid="Player-1-123",sessionNonce=nonce,
                            reloadNonce=changedQueue==true and string.rep("c",32) or reloadNonce,
                            code=code,codeBytes=#code,codeAdler32=ns.CaptureWriter.DigestBytes(code),codeSHA256=changedQueue=="code" and string.rep("e",64) or string.rep("d",64),
                        }}}
                        if changedQueue=="retired" then ns.ProbeDefinitions.entries["Reload-A"]=nil end
                    end
                    assert(loadfile(root.."/"..line))("Lychee Dev",ns)
                end
            end
            toc:close()
            ns.ReceiptView={Hide=function() shown=nil end,Show=function(value) shown=value;return true end}
            local loader=frames[#frames]
            loader.callback(loader,"ADDON_LOADED","Lychee Dev")
            return ns,loader
        end
        local ns,loader=loadRuntime(preparing and "retired")
        if preparing then
            local command="bridge prepare Reload-A "..reloadNonce
            assert(ns.Controls.Handle(command)==nil and reloads==0)
            assert(ns.Controls.Handle("bridge on"))
            assert(ns.Controls.Handle("bridge bind "..nonce))
            assert(ns.Reentry.LoadQueue("Reload-A",secret)==nil and reloads==0)
            assert(ns.ReportStore.Commit("Reload-A",code,{value=42}))
            assert(ns.Controls.Handle(command)==nil and reloads==0 and LycheeToolkitDB.reentry==nil)
            LycheeToolkitDB.reports["Reload-A"]=nil
            assert(ns.ProbeRunner.Load("Reload-A",code,reloadNonce))
            assert(ns.Controls.Handle(command)==nil and reloads==0 and LycheeToolkitDB.reentry==nil)
            assert(loadfile(root.."/Bridge/ProbeRunner.lua"))("Lychee Dev",ns)
            InCombatLockdown=function() return true end
            assert(ns.Controls.Handle(command)==nil and reloads==0 and LycheeToolkitDB.reentry==nil)
            InCombatLockdown=function() return false end
            assert(ns.Controls.Handle(command)=="reload_requested")
            assert(reloads==1 and LycheeToolkitDB.reentry.queueReload==true and reentryExecutions==0)
            assert(ns.Controls.Handle(command)==nil and reloads==1)
            ns,loader=loadRuntime(mode=="prepare-missing" and "retired" or mode=="prepare-nonce")
            if mode=="prepare-actor" then actor.guid="Player-1-other" end
            loader.callback(loader,"LOADING_SCREEN_DISABLED")
            loader.callback(loader,"PLAYER_ENTERING_WORLD",false,mode~="prepare-login")
            assert(next(loader.events)==nil and loader.callback==nil and LycheeToolkitDB.reentry==nil)
            if mode=="prepare" then
                assert(ns.Session.Current().runtimeEpoch==2 and shown)
                outputs[#outputs+1]=shown
                assert(ns.ProbeRunner.VerifyAbsent("Reload-A"))
                assert(ns.ReportStore.Read("Reload-A")==nil)
                assert(ns.Controls.Handle("bridge load Reload-A"))
            else assert(ns.Session.Current()==nil) end
            assert(reentryExecutions==0 and reloads==1 and #frames==2)
        else
        assert(#frames==1 and next(loader.events)==nil)
        assert(ns.Controls.Handle("bridge reload Reload-A")==nil and reloads==0)
        assert(ns.Controls.Handle("bridge on"))
        assert(ns.Controls.Handle("bridge bind "..nonce))
        assert(ns.Controls.Handle("bridge load Reload-A"))
        assert(ns.Controls.Handle("bridge reload Reload-A")==nil and reloads==0)
        local report=assert(ns.Controls.Handle("bridge run Reload-A"))
        local cleanupCommand="bridge clean Reload-A "..string.rep("e",32)
        assert(ns.Controls.Handle(cleanupCommand)==nil and reloads==0, "unacknowledged cleanup")
        InCombatLockdown=function() return true end
        assert(ns.Controls.Handle("bridge reload Reload-A")==nil and reloads==0 and LycheeToolkitDB.reentry==nil)
        InCombatLockdown=function() return false end
        local reloadAPI=C_UI
        C_UI=nil
        assert(ns.Controls.Handle("bridge reload Reload-A")==nil and reloads==0 and LycheeToolkitDB.reentry==nil)
        C_UI=reloadAPI
        local storedBody=LycheeToolkitDB.reports["Reload-A"].body
        LycheeToolkitDB.reports["Reload-A"].body="changed-before-submit"
        assert(ns.Controls.Handle("bridge reload Reload-A")==nil and reloads==0 and LycheeToolkitDB.reentry==nil)
        LycheeToolkitDB.reports["Reload-A"].body=storedBody
        local savedReport=LycheeToolkitDB.reports["Reload-A"]
        local command="bridge reload Reload-A"
        if cleanup then
            -- Exercise the real ordering: report flush and reentry, then ACK,
            -- then a second reload which flushes deletion and unloads the queue.
            assert(ns.Controls.Handle(command)=="reload_requested")
            ns,loader=loadRuntime()
            loader.callback(loader,"LOADING_SCREEN_DISABLED")
            loader.callback(loader,"PLAYER_ENTERING_WORLD",false,true)
            assert(ns.Session.Current().runtimeEpoch==2 and ns.ReportStore.Read("Reload-A")==report)
            local sequence=assert(report:match('"sequence":(%d+)'))
            assert(ns.Controls.Handle("bridge ack Reload-A "..sequence))
            assert(ns.ReportStore.Acknowledged("Reload-A"))
            assert(ns.Reentry.Reload("Reload-A","invalid")==nil and reloads==1 and LycheeToolkitDB.reentry==nil)
            command=cleanupCommand
        end
        local value,failure=ns.Controls.Handle(command)
        if mode=="submission" then assert(value==nil and failure=="reload_submission_failed")
        else assert(value=="reload_requested") end
        local expectedReloads=cleanup and 2 or 1
        assert(reloads==expectedReloads and LycheeToolkitDB.reentry and reentryExecutions==1)
        assert(ns.Controls.Handle(command)==nil and reloads==expectedReloads)
        if mode=="disabled" then LycheeToolkitDB.options.bridgeEnabled=nil end
        if mode=="future" then LycheeToolkitDB.reentry.schema="future" end
        if mode=="cleanup-report" then LycheeToolkitDB.reports["Reload-A"]=savedReport end
        ns,loader=loadRuntime(cleanup and mode~="cleanup-queue" and "retired" or mode=="queue" or (mode=="code" and "code"))
        assert(#frames==expectedReloads+1 and ns.Session.Current()==nil)
        if mode=="disabled" or mode=="future" then
            assert(next(loader.events)==nil and loader.callback==nil and LycheeToolkitDB.reentry)
            assert(ns.Controls.Handle("bridge off"))
            if mode=="disabled" then assert(LycheeToolkitDB.reentry==nil)
            else assert(LycheeToolkitDB.reentry.schema=="future") end
        else
            assert(loader.events.PLAYER_ENTERING_WORLD)
            local callback=loader.callback
            if mode=="actor" then actor.guid="Player-1-other" end
            if mode=="epoch" then LycheeToolkitDB.runtimeEpoch=9 end
            if mode=="body" then LycheeToolkitDB.reports["Reload-A"].body="changed" end
            local replacement
            if mode=="replace" then replacement={schema="foreign"};LycheeToolkitDB.reentry=replacement end
            if mode=="edit" then LycheeToolkitDB.reentry.reloadNonce=string.rep("e",32) end
            if mode=="off" then assert(ns.Controls.Handle("bridge off")) end
            if mode=="world-first" or mode=="world-exit" then
                callback(loader,"PLAYER_ENTERING_WORLD",false,true)
                assert(ns.Session.Current()==nil and shown==nil and LycheeToolkitDB.reentry,"premature reentry")
                if mode=="world-exit" then callback(loader,"PLAYER_LEAVING_WORLD") end
            elseif mode=="loading-restart" then
                callback(loader,"LOADING_SCREEN_DISABLED")
                callback(loader,"LOADING_SCREEN_ENABLED")
                callback(loader,"PLAYER_ENTERING_WORLD",false,true)
                assert(ns.Session.Current()==nil and shown==nil,"old loading completion reused")
            end
            callback(loader,"LOADING_SCREEN_DISABLED")
            callback(loader,"PLAYER_ENTERING_WORLD",mode=="initial",mode=="secret" and secret or mode~="login")
            assert(next(loader.events)==nil and loader.callback==nil)
            local restored=mode=="success" or mode=="submission" or mode=="cleanup" or mode=="world-first" or mode=="loading-restart"
            if restored then
                assert(ns.Session.Current().runtimeEpoch==expectedReloads+1 and shown and LycheeToolkitDB.reentry==nil)
                if cleanup then
                    assert(ns.ReportStore.Read("Reload-A")==nil)
                    assert(ns.ReportStore.Acknowledged("Reload-A")==nil, "ACK runtime state inherited")
                else assert(ns.ReportStore.Read("Reload-A")==report) end
                if mode=="success" or mode=="cleanup" then outputs[#outputs+1]=shown end
            else
                assert(ns.Session.Current()==nil)
                if mode=="replace" then assert(LycheeToolkitDB.reentry==replacement)
                elseif mode=="edit" then assert(LycheeToolkitDB.reentry.reloadNonce==string.rep("e",32))
                else assert(LycheeToolkitDB.reentry==nil) end
            end
            callback(loader,"PLAYER_ENTERING_WORLD",false,true)
            assert(reentryExecutions==1 and reloads==expectedReloads)
        end
        end
    end
end
local ns={}
assert(loadfile(root.."/Bridge/CaptureWriter.lua"))("Lychee Dev",ns)
io.write(assert(ns.CaptureWriter.Encode(outputs)))
