local root, payloadPath = assert(arg[1]), assert(arg[2])
local input = assert(io.open(payloadPath, "rb"))
local payload = input:read("*a"); input:close()
local ns, frames, session = {}, {}, nil
local secret = {}
issecretvalue = function(value) return rawequal(value, secret) end
local scale = 1.25
UIParent = { GetEffectiveScale = function() return scale end }
local cancellations = 0
ns.Session = { Current = function() return session, "session_unbound" end,
    CancelInputWait = function() cancellations = cancellations + 1 end }
local callbacks, registered, removed = {}, 0, 0
EventRegistry = {
    RegisterCallback = function(_, event, callback, owner)
        assert(event == "ChatFrame.OnEditBoxFocusGained" and callbacks[owner] == nil)
        callbacks[owner] = callback; registered = registered + 1
    end,
    UnregisterCallback = function(_, event, owner)
        assert(event == "ChatFrame.OnEditBoxFocusGained" and callbacks[owner])
        callbacks[owner] = nil; removed = removed + 1
    end,
}
CreateFrame = function(kind, name, parent)
    assert(kind == "Frame" and name == nil and parent == UIParent)
    local frame = { textures = {}, events = {} }
    function frame:Hide() self.visible = false end
    function frame:Show() self.visible = true end
    function frame:EnableMouse(value) assert(value == false) end
    function frame:SetFrameStrata(value) assert(value == "DIALOG") end
    function frame:SetPoint(...) self.point = {...} end
    function frame:SetScale(value) self.scale = value end
    function frame:SetSize(w, h) self.width, self.height = w, h end
    function frame:UnregisterAllEvents() self.events = {} end
    function frame:RegisterEvent(event) assert(event == "PLAYER_LEAVING_WORLD" or event == "PLAYER_REGEN_DISABLED" or event == "LOADING_SCREEN_ENABLED"); self.events[event] = true end
    function frame:SetScript(event, fn) assert(event == "OnEvent"); self.callback = fn end
    function frame:CreateTexture(_, layer)
        local texture = { layer = layer }
        function texture:SetColorTexture(r, g, b, a) self.black = r == 0; assert(r == g and g == b and a == 1) end
        function texture:SetAllPoints(target) assert(target == frame) end
        function texture:ClearAllPoints() end
        function texture:SetPoint(anchor, target, relative, x, y)
            assert(anchor == "TOPLEFT" and target == frame and relative == "TOPLEFT")
            self.x, self.y = x, -y
        end
        function texture:SetSize(w, h) self.width, self.height = w, h end
        function texture:Show() self.visible = true end
        function texture:Hide() self.visible = false end
        self.textures[#self.textures + 1] = texture
        return texture
    end
    frames[#frames + 1] = frame
    return frame
end
for _, name in ipairs({"CaptureWriter", "MatrixSymbol", "ReceiptView"}) do
    assert(loadfile(root .. "/Bridge/" .. name .. ".lua"))("Lychee Dev", ns)
end
assert(#frames == 0)
assert(ns.ReceiptView.Show(payload) == nil and #frames == 0)
assert(ns.MatrixSymbol.Encode(secret) == nil and ns.MatrixSymbol.Encode("") == nil)
assert(ns.MatrixSymbol.Encode(string.rep("x", 2049)) == nil)
session = { generation = 1 }
assert(ns.ReceiptView.Show(secret) == nil and #frames == 0)
assert(ns.ReceiptView.Show(string.rep("x", 2049)) == nil and #frames == 0)
assert(ns.ReceiptView.Show(payload))
assert(#frames == 1)
local frame = frames[1]
assert(frame.visible and frame.scale == nil)
local allocated = #frame.textures
assert(ns.ReceiptView.Show(payload) and #frame.textures == allocated)
assert(registered == 1 and removed == 0)
assert(frame.events.PLAYER_LEAVING_WORLD and frame.events.PLAYER_REGEN_DISABLED)
local function capture()
    local runs = {}
    for _, texture in ipairs(frame.textures) do
        if texture.black and texture.visible then
            runs[#runs + 1] = table.concat({texture.x, texture.y, texture.width, texture.height}, ",")
        end
    end
    return { width = frame.width, height = frame.height, runs = runs }
end
local first = capture()
assert(ns.ReceiptView.Show("small"))
local second = capture()
local pairReceipt = assert(ns.CaptureWriter.EncodeSignal({schema="lycheedev.signal.v1",kind="reported",
 release="2.0.2",product="classic",build="5.5.4.69934",requestId="R-symbol",sequence=8,
 inputReady=false,reportBytes=13,reportAdler32="209c046d"}))
local readyBytes, _, readySignal = ns.CaptureWriter.EncodeSignal({schema="lycheedev.signal.v1",kind="ready",
 release="2.0.2",product="classic",build="5.5.4.69934",requestId="",sequence=9,
 sessionNonce=string.rep("a",32),runtimeEpoch=2,inputReady=true})
local pairBytes = assert(ns.CaptureWriter.EncodeReceiptPair(pairReceipt,readySignal))
assert(ns.ReceiptView.Show(pairReceipt, readyBytes, nil, readySignal))
local paired = capture()
allocated = #frame.textures
local registrations = registered
assert(ns.ReceiptView.Show(pairReceipt, readyBytes, nil, readySignal) and #frame.textures == allocated and registered == registrations)
assert(ns.ReceiptView.Show(payload, secret) == nil and not frame.visible)
assert(ns.ReceiptView.Show(pairReceipt, readyBytes, nil, readySignal))
local _, staleFocus = next(callbacks)
local before = cancellations
staleFocus()
assert(not frame.visible and frame.callback == nil and next(frame.events) == nil and next(callbacks) == nil)
assert(cancellations == before + 1)
assert(ns.ReceiptView.Show(payload) and #frames == 1)
staleFocus()
assert(frame.visible)
frame.callback(frame, "PLAYER_REGEN_DISABLED")
assert(not frame.visible and next(callbacks) == nil and next(frame.events) == nil)
assert(ns.ReceiptView.Show(payload))
frame.callback(frame, "PLAYER_LEAVING_WORLD")
assert(not frame.visible and next(callbacks) == nil)
assert(ns.ReceiptView.Show(payload))
ns.ReceiptView.Hide()
assert(not frame.visible and next(frame.events) == nil and next(callbacks) == nil)
assert(registered == removed)
local registry = EventRegistry
EventRegistry = nil
local shown, failure = ns.ReceiptView.Show(payload)
assert(shown == nil and failure == "receipt_events_unavailable" and not frame.visible)
EventRegistry = registry
scale = secret
assert(ns.ReceiptView.Show(payload) == nil and not frame.visible)
scale = 1
session = nil
assert(ns.ReceiptView.Show(payload) == nil and next(frame.events) == nil)
io.write(assert(ns.CaptureWriter.Encode({first=first, second=second, paired=paired,pairBytes=pairBytes})))
