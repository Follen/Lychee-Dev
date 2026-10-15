local root=assert(arg[1])
for _,scenario in ipairs({"disabled","login","world-first","loading-first","wake","expiry","combat"}) do
    local now,frames,timers=0,{},{}
    GetTime=function() return now end
    UIParent={}
    C_Timer={NewTimer=function(delay,callback)
        local timer={at=now+delay,callback=callback}
        function timer:Cancel() self.cancelled=true end
        timers[#timers+1]=timer; return timer
    end}
    local function advance(to)
        while true do
            local nextTimer
            for _,timer in ipairs(timers) do
                if not timer.cancelled and not timer.fired and timer.at<=to and (not nextTimer or timer.at<nextTimer.at) then nextTimer=timer end
            end
            if not nextTimer then break end
            now=nextTimer.at; nextTimer.fired=true; nextTimer.callback()
        end
        now=to
    end
    CreateFrame=function()
        local f={events={},textures={}}
        function f:Hide() self.shown=false end
        function f:Show() self.shown=true end
        function f:IsShown() return self.shown end
        function f:SetScript(key,fn) assert(key=="OnEvent"); self.callback=fn end
        function f:RegisterEvent(key) self.events[key]=true end
        function f:UnregisterAllEvents() self.events={} end
        function f:EnableMouse(value) assert(value==false) end
        function f:SetFrameStrata() end
        function f:SetPoint() end
        function f:SetSize() end
        function f:CreateTexture()
            local t={}
            function t:SetAllPoints() end
            function t:SetSize() end
            function t:SetPoint() end
            function t:SetColorTexture(r,g,b) self.color={r,g,b} end
            self.textures[#self.textures+1]=t; return t
        end
        frames[#frames+1]=f; return f
    end
    local ns={Persistence={BridgeEnabled=function() return scenario~="disabled" end},Compat={GetPhysicalPixelSize=function() return 1 end},
        ReceiverBindings={Current=function() return {} end},Platform={ObserveInputState=function() return true end}}
    assert(loadfile(root.."/Bridge/StartupBeacon.lua"))("Lychee Dev",ns)
    assert(#frames==0 and #timers==0)
    ns.StartupBeacon.Arm()
    if scenario=="disabled" then assert(#frames==0 and #timers==0)
    else
        local f=frames[1]
        assert(#frames==1 and not f.shown)
        local event=f.callback
        if scenario=="login" then event(f,"PLAYER_ENTERING_WORLD",true,false); assert(not f.shown and not f.callback)
        else
            if scenario=="world-first" then
                event(f,"PLAYER_ENTERING_WORLD",false,true); assert(not f.shown); event(f,"LOADING_SCREEN_DISABLED")
            else
                event(f,"LOADING_SCREEN_DISABLED"); assert(not f.shown); event(f,"PLAYER_ENTERING_WORLD",false,true)
            end
            assert(f.shown and #f.textures==2 and f.textures[2].color[1]==1,"expected one RGB patch and its black surround")
            advance(.5); assert(f.textures[2].color[2]==1)
            advance(1); assert(f.textures[2].color[3]==1)
            if scenario=="wake" then ns.StartupBeacon.Stop()
            elseif scenario=="combat" then event(f,"PLAYER_REGEN_DISABLED")
            else advance(44.9); assert(f.shown); advance(45) end
            assert(not f.shown and not f.callback and next(f.events)==nil)
            local timerCount=#timers
            advance(100); ns.StartupBeacon.Arm()
            assert(#timers==timerCount and not f.shown,"stopped beacon revived")
        end
    end
end
print("startup beacon: bounded reload hint passed")
