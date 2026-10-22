local Trace = { a = 0, b = 0 }
local function SetA()
    Trace.a = "yes"
    return true
end
local function SetB()
    Trace.b = "yes"
    return "done"
end
local selectedOr = SetA() or SetB()
local selectedAnd = false and SetB()
local zeroTruthy = 0 or SetB()
local beforeRight = Trace.b
local selectedRight = true and SetB()
local afterRight = Trace.b
local observedAnd = selectedAnd
local observedZero = zeroTruthy
