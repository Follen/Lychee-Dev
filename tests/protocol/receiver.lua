local root = assert(arg[1])
local golden = assert(arg[2])
local ns = {Release="3.0.0", Startup={ready=true, identity={product="retail",build="12.1.0.69933"}}}
ns.L = {RECEIVER_READY="Ready", RECEIVER_STAGED="Staged", RECEIVER_VERIFY="Verify",
    RECEIVER_ACCEPTED="Accepted", RECEIVER_REJECTED="Rejected",
    RECEIVER_CANCELLED="Cancelled", RECEIVER_TIMEOUT="Timeout",
    RECEIVER_TITLE="Developer connection", RECEIVER_BUDGET="%d s left",
    RECEIVER_INPUT_HINT="Receiving a managed command"}
ns.Theme = {window={.055,.055,.063}, field={.085,.085,.095},
    fieldBorder={.4,.4,.42}, text={.94,.932,.91}, textMuted={.71,.705,.69},
    textDim={.59,.58,.59}, warning={.88,.645,.25}, danger={.895,.3,.315},
    success={.31,.69,.455}, font="font",
    SetFont=function() end}
ns.Widgets = {LOGO_TEXTURE="logo"}
GetTime = function() return 0 end
local frames, bindings, timers, shown, calls = {}, {}, {}, {}, {}
local visible
local businessReceipt = '{"schema":"lycheedev.signal.v1","kind":"reported",' ..
    '"sessionNonce":"' .. string.rep("2",32) .. '","requestId":"OP-one",' ..
    '"runtimeEpoch":1,"sequence":1,"inputReady":false}'
UIParent = {}
local secret={}
issecretvalue = function(value) return rawequal(value,secret) end
local combat=true
InCombatLockdown = function() return combat end
GetBindingAction = function(chord, override)
    return override and bindings[chord] and ("CLICK " .. bindings[chord] .. ":LeftButton") or ""
end
SetOverrideBindingClick = function(_, _, chord, button) bindings[chord] = button end
ClearOverrideBindings = function() bindings = {} end
local function object(kind, name)
    local value = {kind=kind,name=name,scripts={},text="",focus=false}
    function value:SetScript(key, callback) self.scripts[key]=callback end
    function value:SetText(text)
        self.text=text
        if self.kind=="EditBox" and self.scripts.OnTextChanged then self.scripts.OnTextChanged(self) end
    end
    function value:GetText() return self.text end
    function value:SetFocus() self.focus=true end
    function value:HasFocus() return self.focus end
    function value:ClearFocus()
        local had=self.focus; self.focus=false
        if had and self.scripts.OnEditFocusLost then self.scripts.OnEditFocusLost(self) end
    end
    function value:GetName() return self.name end
    function value:GetFrameLevel() return 1 end
    function value:Show() self.visible=true end
    function value:Hide() self.visible=false end
    function value:CreateTexture() return object("Texture") end
    function value:CreateFontString() return object("FontString") end
    function value:EnableMouse(enabled) self.mouse=enabled end
    function value:EnableMouseWheel(enabled) self.wheel=enabled end
    for _, method in ipairs({"SetAllPoints","SetFrameStrata","SetFrameLevel",
        "SetSize","SetWidth","SetPoint","SetColorTexture","SetAutoFocus","SetMaxLetters",
        "RegisterEvent","UnregisterAllEvents","RegisterForClicks",
        "SetBackdrop","SetBackdropColor","SetBackdropBorderColor","SetTexCoord",
        "SetTexture","SetVertexColor","SetJustifyH","SetFont","SetShadowOffset","SetTextColor","SetAlpha","ClearAllPoints"}) do
        value[method]=function() end
    end
    frames[#frames+1]=value
    return value
end
CreateFrame=object
C_Timer={NewTimer=function(seconds, callback)
    assert(seconds==20)
    local timer={callback=callback, cancelled=false}
    function timer:Cancel() self.cancelled=true end
    timers[#timers+1]=timer
    return timer
end}
ns.Persistence={Current=function() return LycheeToolkitDB end,Bridge=function() return LycheeToolkitBridgeDB end}
local currentSession
ns.Session={Current=function() return currentSession end}
ns.Platform={ObserveActor=function() return {character="Paladin",realm="Realm",guid="Player-1-123"} end}
ns.ReceiptView={
    AnchorCompanion=function() return true end,
    ShowIdentity=function(text)
        assert(ns.MatrixSymbol.Encode(text),"receiver receipt exceeded optical encoder")
        shown[#shown+1]=text; visible=text; return true
    end,
    Show=function(text) visible=text; return true end,
    Current=function() return visible end,
    Hide=function() visible=nil end,
}
ns.Controls={Handle=function(text)
    assert(not ns.Receiver.IsActive(),"business entered while receiver active")
    for _,value in ipairs(frames) do
        if value.kind=="EditBox" then assert(not value:HasFocus(),"business entered with input focus") end
        if value.kind=="Frame" and value.scripts.OnMouseDown then
            assert(not value.visible and not value.scripts.OnUpdate,"business entered behind receiver shield")
        end
    end
    assert(timers[#timers].cancelled,"receiver deadline outlived submission")
    calls[#calls+1]=text; visible=businessReceipt; return businessReceipt
end}
LycheeToolkitDB={schema=1}
LycheeToolkitBridgeDB={schema=1}
for _, path in ipairs({"UI/Theme.lua","Bridge/CaptureWriter.lua","Bridge/MatrixSymbol.lua",
    "Bridge/InputProtocol.lua","Bridge/ReceiverBindings.lua","Bridge/Receiver.lua"}) do
    assert(loadfile(root.."/"..path))("Lychee Dev",ns)
end
local goldenCount=0
for line in io.lines(golden) do
    local name,requestId,action,argument,valid=string.match(line,
        '"name":"([^"]+)","requestId":"([^"]+)","action":"([^"]+)","arg":"([^"]+)","valid":(true|false)')
    -- Lua patterns have no alternation, so read the boolean separately.
    if not name then
        name,requestId,action,argument=string.match(line,
            '"name":"([^"]+)","requestId":"([^"]+)","action":"([^"]+)","arg":"([^"]+)"')
        valid=string.match(line,'"valid":(true)') or string.match(line,'"valid":(false)')
    end
    if name then
        goldenCount=goldenCount+1
        local body=ns.InputProtocol.Body(action,requestId,argument)
        local prefix=table.concat({"LDB1",string.rep("a",32),"1",requestId,string.rep("b",16),action,argument,tostring(#body)},":")
        local wire=prefix..":"..ns.CaptureWriter.DigestBytes(prefix)
        local parsed=ns.InputProtocol.Stage(wire,string.rep("a",32),1)
        assert((parsed~=nil)==(valid=="true"),"golden drift: "..name)
    end
end
assert(goldenCount>=20,"receiver golden fixtures not read")
assert(#frames==0,"receiver built UI at module load")
assert(ns.Receiver.Register())
assert(#frames==1,"binding owner should defer protected setup during combat")
combat=false
assert(frames[1].scripts.OnEvent)
frames[1].scripts.OnEvent(frames[1],"PLAYER_REGEN_ENABLED")
assert(#frames==4,"only owner and three binding buttons should exist cold")
local function editor()
    for _, value in ipairs(frames) do if value.kind=="EditBox" then return value end end
end
local function field(name)
    return string.match(shown[#shown], '"'..name..'":"([^"]+)"')
end
local function send(text)
    local edit=assert(editor())
    local before=#calls
    for i=1,#text do edit:SetText(edit:GetText()..string.sub(text,i,i)) end
    if string.sub(text,1,5)=="LDC1:" then
        assert(#calls==before,"text change dispatched business outside a key event")
        edit.scripts.OnEnterPressed(edit)
    end
end
local function stage(action, requestId, argument, attempt)
    local nonce, epoch=assert(field("receiverNonce")), assert(tonumber(string.match(shown[#shown], '"runtimeEpoch":([0-9]+)')))
    local body=ns.InputProtocol.Body(action,requestId,argument)
    local prefix=table.concat({"LDB1",nonce,tostring(epoch),requestId,attempt,action,argument,tostring(#body)},":")
    return prefix..":"..assert(ns.CaptureWriter.DigestBytes(prefix)),body
end
local function commit(nonce,attempt,challenge,digest)
    local prefix=table.concat({"LDC1",nonce,attempt,challenge,digest},":")
    return prefix..":"..assert(ns.CaptureWriter.DigestBytes(prefix))
end
local attempt=string.rep("a",16)
assert(ns.Receiver.Wake())
assert(string.find(shown[#shown],'"kind":"receiver_ready"',1,true))
assert(not string.find(shown[#shown], '"priorSessionSequence"', 1, true)
    and not string.find(shown[#shown], '"priorSessionEpoch"', 1, true),
    "a no-session ready advertised a prior session baseline")
local firstNonce=field("receiverNonce")
local wire,body=stage("run","OP-one","-",attempt)
send(wire)
assert(string.find(shown[#shown],'"kind":"receiver_staged"',1,true) and #calls==0)
local bodyDigest=assert(ns.CaptureWriter.DigestBytes(body))
assert(field("bodyAdler32")==bodyDigest)
assert(ns.Receiver.Submit())
assert(string.find(shown[#shown],'"kind":"receiver_commit_ready"',1,true) and #calls==0)
editor():SetText("]") -- WM_CHAR may follow the submit key-down after focus.
assert(string.find(shown[#shown],'"kind":"receiver_commit_ready"',1,true))
local firstChallenge=assert(field("commitNonce"))
local firstCommit=commit(firstNonce,attempt,firstChallenge,bodyDigest)
send(firstCommit)
assert(#calls==1 and calls[1]=="bridge run OP-one")
assert(string.find(shown[#shown],'"kind":"receiver_accepted"',1,true))
assert(visible == businessReceipt, "business producer did not replace accepted")
assert(ns.Receiver.Submit() == nil and #calls == 1,
    "late submit changed completed delivery")
assert(visible == businessReceipt, "late submit replaced business evidence")
assert(not ns.Receiver.IsActive() and not editor():HasFocus(),
    "business dispatch retained input resources")
assert(ns.Receiver.Wake(), "business completion requires an input cleanup transaction")
local newNonce=field("receiverNonce")
assert(newNonce~=firstNonce)
local secondAttempt=string.rep("b",16)
local secondWire,secondBody=stage("run","OP-two","-",secondAttempt)
send(secondWire)
-- A delayed old key may mint a challenge, but cannot execute the new stage.
assert(ns.Receiver.Submit() and #calls==1)
send(firstCommit)
assert(#calls==1 and string.find(shown[#shown],'"kind":"receiver_rejected"',1,true),
    "stale commit executed a new request")
assert(ns.Receiver.Wake())
local thirdWire=stage("run","OP-two","-",string.rep("c",16))
send(string.sub(thirdWire,1,-2).."0")
assert(#calls==1 and string.find(shown[#shown],'"kind":"receiver_rejected"',1,true),
    "checksum pollution executed")
assert(ns.Receiver.Wake())
local timer=timers[#timers]
timer.callback()
assert(string.find(shown[#shown],'"kind":"receiver_timeout"',1,true))
assert(ns.Receiver.Wake())
local edit=assert(editor())
local shield
for _, value in ipairs(frames) do
    if value.kind=="Frame" and value.scripts.OnMouseDown then shield=value end
end
assert(shield and shield.visible and shield.mouse and shield.wheel,"receiving did not block mouse and wheel")
edit.focus=false
shield.scripts.OnMouseDown(shield,"LeftButton")
assert(edit:HasFocus() and ns.Receiver.IsActive(),"ordinary click interrupted command reception")
edit:ClearFocus()
assert(ns.Receiver.IsActive() and edit:HasFocus(),"incidental focus loss cancelled the input shield")
assert(ns.Receiver.Close())
assert(not shield.visible and not edit:HasFocus(),"close retained mouse or keyboard ownership")
assert(#calls==1)
assert(ns.Receiver.Wake())
for _, value in ipairs(frames) do
    if value.kind=="Frame" and value.scripts.OnEvent then
        value.scripts.OnEvent(value,"PLAYER_REGEN_DISABLED")
    end
end
assert(string.find(shown[#shown],'"errorCode":"receiver_combat"',1,true))
assert(ns.Receiver.Wake())
local secretWire,secretBody=stage("load","OP-secret","-",string.rep("d",16))
send(secretWire)
assert(ns.Receiver.Submit())
ns.Controls.Handle=function() return nil,secret end
send(commit(field("receiverNonce"),string.rep("d",16),field("commitNonce"),
    ns.CaptureWriter.DigestBytes(secretBody)))
assert(string.find(shown[#shown],'"errorCode":"receiver_dispatch_failed"',1,true),
    "secret failure was inspected before secrecy guard")
assert(ns.Receiver.Submit() == nil,
    "inactive submit must not redisplay transport receipts")
assert(not ns.Receiver.IsActive(), "dispatch failure retained input")
local redispatched = 0
ns.Controls.Handle=function() redispatched=redispatched+1; error("replayed business request") end
assert(ns.Receiver.Wake())
local repeatedWire,repeatedBody=stage("load","OP-secret","-",string.rep("e",16))
send(repeatedWire)
assert(ns.Receiver.Submit())
send(commit(field("receiverNonce"),string.rep("e",16),field("commitNonce"),
    ns.CaptureWriter.DigestBytes(repeatedBody)))
assert(redispatched==0,"a failed accepted request was redispatched on fresh wake")
assert(string.find(shown[#shown],'"errorCode":"receiver_dispatch_failed"',1,true),
    "accepted replay lost the original dispatch failure across wakes")
assert(ns.Receiver.Close())

assert(ns.ReceiverBindings.Configure("close","ALT-CTRL-F12"))
assert(ns.ReceiverBindings.Configure("wake","ALT-CTRL-["))
assert(ns.ReceiverBindings.MatchKey("[", "wake"))
assert(ns.Receiver.Wake())
editor():SetText("[") -- Late custom wake WM_CHAR must not reject the ready phase.
assert(editor():GetText()=="", "custom wake punctuation was not stripped")
assert(string.find(shown[#shown],'"kind":"receiver_ready"',1,true))
local customWire=stage("run","OP-custom","-",string.rep("f",16))
send(customWire)
assert(ns.Receiver.Submit())
editor():SetText("]") -- Submit still uses ], independent of the wake terminal.
assert(string.find(shown[#shown],'"kind":"receiver_commit_ready"',1,true))
assert(ns.Receiver.Close())
assert(ns.ReceiverBindings.Configure("wake","ALT-CTRL-F1"))
assert(ns.Receiver.Wake())
local fkeyWire=stage("run","OP-fkey","-",string.rep("1",16))
send(fkeyWire)
assert(ns.Receiver.Submit())
editor():SetText("]")
assert(string.find(shown[#shown],'"kind":"receiver_commit_ready"',1,true),
    "submit punctuation was rejected after a function-key wake")
assert(ns.Receiver.Close())
currentSession = {sessionNonce=string.rep("2",32), runtimeEpoch=1, sequence=23}
assert(ns.Receiver.Wake())
assert(field("sessionNonce")==currentSession.sessionNonce,
    "receiver_ready omitted the active session identity")
assert(string.find(shown[#shown], '"priorSessionSequence":23', 1, true)
    and string.find(shown[#shown], '"priorSessionEpoch":1', 1, true),
    "receiver_ready omitted the pre-commit session baseline")
assert(ns.Receiver.Close())
local cancelledReceipt = visible
assert(string.find(cancelledReceipt, '"kind":"receiver_cancelled"', 1, true),
    "closing active input did not cancel it")
assert(ns.Receiver.Close() and visible == cancelledReceipt,
    "closing inactive input changed business display")

local directReceipt = string.gsub(businessReceipt, "OP%-one", "OP-direct")
ns.Controls.Handle = function(text)
    calls[#calls + 1] = text
    visible = directReceipt
    return directReceipt
end
assert(ns.Receiver.Wake())
local directNonce = field("receiverNonce")
local directWire, directBody = stage("run", "OP-direct", "-", string.rep("3",16))
send(directWire)
assert(ns.Receiver.Submit())
send(commit(directNonce, string.rep("3",16), field("commitNonce"),
    ns.CaptureWriter.DigestBytes(directBody)))
assert(visible == directReceipt, "direct business card was not visible")
local beforeCloseCalls = #calls
assert(ns.Receiver.Close())
assert(visible == directReceipt and #calls == beforeCloseCalls,
    "input close must not rewrite or redispatch business")
assert(ns.Receiver.Close() and visible == directReceipt,
    "repeat input close changed business display")
assert(ns.Receiver.Wake() and field("receiverNonce") ~= directNonce,
    "verified Close did not allow a fresh receiver nonce")
assert(ns.Receiver.Close())

ns.Controls.Handle = function(text)
    calls[#calls + 1] = text
    assert(text == "bridge hide")
    return nil, "receipt_busy"
end
assert(ns.Receiver.Wake())
local failedHideNonce = field("receiverNonce")
local failedHideWire, failedHideBody = stage("hide", "-", "-", string.rep("5",16))
send(failedHideWire)
assert(ns.Receiver.Submit())
send(commit(failedHideNonce, string.rep("5",16), field("commitNonce"),
    ns.CaptureWriter.DigestBytes(failedHideBody)))
assert(string.find(visible, '"errorCode":"receipt_busy"', 1, true),
    "failed hide lost its dispatcher error")
assert(ns.Receiver.Wake(), "dispatch failure sealed subsequent delivery")
assert(ns.Receiver.Close())

ns.Controls.Handle = function(text)
    calls[#calls + 1] = text
    assert(text == "bridge hide")
    visible = nil
    return true
end
assert(ns.Receiver.Wake())
local hideNonce = field("receiverNonce")
local hideWire, hideBody = stage("hide", "-", "-", string.rep("4",16))
send(hideWire)
assert(ns.Receiver.Submit())
send(commit(hideNonce, string.rep("4",16), field("commitNonce"),
    ns.CaptureWriter.DigestBytes(hideBody)))
assert(visible == nil, "successful hide left a receiver QR")
assert(ns.Receiver.Close() and visible == nil,
    "CLOSE after hide recreated a receiver QR")
assert(ns.Receiver.Wake() and field("receiverNonce") ~= hideNonce,
    "hide did not clear the accepted seal")
assert(ns.Receiver.Close())
io.write("receiver: incremental framing, challenge, stale commit, independent delivery, timeout, focus passed")
