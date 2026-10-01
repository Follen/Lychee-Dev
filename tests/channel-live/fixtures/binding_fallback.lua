-- Opt-in real-client fixture: force one temporary native-key conflict without
-- changing account/character bindings. The next host transaction must use F11.
local probe=...
local ns=LycheeDevInternal
assert(not InCombatLockdown(),"fallback fixture requires noncombat")
assert(not probe:IsInputProtected() and not ns.SlotRuntime.IsActive())
local before=assert(ns.ReceiverBindings.Current())
local playerWake=GetBindingAction("ALT-CTRL-F12",false)
local owner=CreateFrame("Frame")
local name="LycheeDevFallbackFixture"..tostring(math.random(1,999999999))
assert(_G[name]==nil,"fixture name already exists")
local button=CreateFrame("Button",name,UIParent)
button:RegisterForClicks("AnyDown")
local clicks=0
button:SetScript("OnClick",function()clicks=clicks+1 end)
assert(probe:OnCleanup(function()
    ClearOverrideBindings(owner)
    button:SetScript("OnClick",nil);button:Hide();owner:Hide()
    assert(GetBindingAction("ALT-CTRL-F12",false)==playerWake,"player binding changed")
end))
assert(SetOverrideBindingClick(owner,true,"ALT-CTRL-F12",name)~=false)
assert(GetBindingAction("ALT-CTRL-F12",true)=="CLICK "..name..":LeftButton")
local fallback,reason=ns.ReceiverBindings.Reset()
assert(fallback,reason)
assert(fallback.wake=="ALT-CTRL-F11" and fallback.submit=="ALT-CTRL-SHIFT-F11"
    and fallback.close=="ALT-CTRL-[","fallback profile was not selected")
ns.InputState.Refresh()
local effective=assert(ns.ReceiverBindings.Current())
assert(effective.wake==fallback.wake and effective.submit==fallback.submit)
assert(clicks==0,"fixture received a physical key")
return {before=before,bindings=effective,playerBindingUnchanged=true,fixtureClicks=clicks}
