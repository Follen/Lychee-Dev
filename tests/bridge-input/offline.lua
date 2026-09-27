local ns = { Config = { token = "0123456789abcdef" } }
local frames, timers, bindings = {}, {}, {}
local now, alt, ctrl, shift, focused = 1234.5, false, false, false, nil
UIParent = {}; SlashCmdList = {}
GetTime = function() return now end
GetBuildInfo = function() return "12.1.0", "69933" end
IsAltKeyDown = function() return alt end
IsControlKeyDown = function() return ctrl end
IsShiftKeyDown = function() return shift end
InCombatLockdown = function() return false end
GetBindingAction = function() return "" end
SetOverrideBindingClick = function(_, _, key, name) bindings[key] = _G[name] end
C_Timer = { NewTimer = function(_, callback)
    local t = { callback = callback, Cancel = function(self) self.cancelled = true end }
    timers[#timers + 1] = t; return t
end }
local noop = function() end
local function visual()
    return setmetatable({}, { __index = function() return noop end })
end
function CreateFrame(kind, name)
    local f = { scripts = {}, events = {}, kind = kind, name = name, text = "" }
    setmetatable(f, { __index = function() return noop end })
    function f:SetScript(event, callback) self.scripts[event] = callback end
    function f:RegisterEvent(event) self.events[event] = true end
    function f:UnregisterEvent(event) self.events[event] = nil end
    function f:UnregisterAllEvents() self.events = {} end
    function f:GetName() return self.name end
    function f:RegisterForClicks(phase) self.clickPhase = phase end
    function f:CreateTexture() return visual() end
    function f:CreateFontString() return visual() end
    function f:Show() self.visible = true end
    function f:Hide() self.visible = false end
    function f:SetFocus() focused = self end
    function f:HasFocus() return focused == self end
    function f:ClearFocus() focused = nil; if self.scripts.OnEditFocusLost then self.scripts.OnEditFocusLost(self) end end
    function f:SetText(text) self.text = text; if self.scripts.OnTextChanged then self.scripts.OnTextChanged(self) end end
    function f:GetText() return self.text end
    if name then _G[name] = f end
    frames[#frames + 1] = f; return f
end
local receipt
ns.MatrixSymbol = { Encode = function(text) receipt = text; return {{1,1,1},{1,-1,1},{1,1,1}} end }
assert(loadfile(arg[1]))("LycheeInputLab", ns)
assert(loadfile(arg[2]))("LycheeInputLab", ns)
local startup = frames[1]
startup.scripts.OnEvent(startup, "ADDON_LOADED", "LycheeInputLab")
startup.scripts.OnEvent(startup, "PLAYER_LOGIN")
local function fields()
    local f = {}; for part in string.gmatch(receipt, "[^|]+") do f[#f + 1] = part end; return f
end
assert(fields()[6] == "idle" and #timers == 0)
assert(#frames == 5, "only startup, QR, and three bindings at startup")
local wake = assert(bindings["ALT-CTRL-]"])
local submit = assert(bindings["ALT-CTRL-SHIFT-]"])
local cancel = assert(bindings["ALT-CTRL-["])
local function click(button)
    assert(button.clickPhase == 'AnyDown')
    button.scripts.OnClick(button)
end
click(wake)
local initial = fields(); assert(initial[6] == "ready" and initial[11] == "1")
local editor = assert(focused)
editor:SetText("]"); assert(editor:GetText() == "", "wake character leaked")
click(wake); assert(#timers == 1 and fields()[7] == initial[7], "wake renewed receiver")
local prefix = "LDIL1:" .. initial[7] .. ":echo_test"
local text = prefix .. ":" .. ns.Protocol.Digest(prefix)
assert(not ns.Protocol.Parse(text .. "extra", initial[7]))
assert(not ns.Protocol.Parse(text, "0000000000000000"))
assert(not ns.Protocol.Parse(string.rep("x",129), initial[7]))
editor:SetText(text)
assert(fields()[6] == "staged")
editor.scripts.OnEnterPressed(); editor.scripts.OnEscapePressed(); editor.scripts.OnTabPressed()
assert(fields()[6] == "staged" and fields()[10] == "0")
editor:SetText(text .. "noise"); assert(editor:GetText() == text)
alt,ctrl,shift = true,true,true
editor.scripts.OnKeyDown(editor,"]")
assert(fields()[6] == "accepted" and fields()[10] == "1" and fields()[11] == "0")
assert(timers[1].cancelled)
click(submit); assert(fields()[10] == "1", "duplicate submission executed")
click(wake); assert(not focused and fields()[6] == "accepted", "completed transaction reopened")
click(cancel)
click(wake); editor:SetText(text); assert(fields()[6] == "ready", "old nonce accepted")
click(submit); assert(fields()[6] == "rejected" and not focused)
click(wake); local oldtimer = timers[#timers]; click(cancel); oldtimer.callback()
assert(not focused and oldtimer.cancelled)
click(wake); timers[#timers].callback(); assert(fields()[6] == "timeout" and not focused)
click(wake); focused:ClearFocus(); assert(fields()[6] == "focus_lost")
for i = 1, 80 do click(wake); click(cancel) end
assert(#LycheeInputLabDB.events == 64 and LycheeInputLabDB.truncated)
print("input lab: protocol, cold wake, focused submit, no normal-key submit, freeze, dedup, nonce, timeout, focus loss, bounded log passed")
