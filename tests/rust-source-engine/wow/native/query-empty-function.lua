---@param x number
local function one(x) end
one(1)
local first, second = one(2)
local scalar = (one(3))
local existing = 7
existing = one(4)
local function forward() return one(5) end
local tailFirst, tailSecond = forward()
local function sink(a, b) return a, b end
local tailArgA, tailArgB = sink(one(6))
local scalarArgA, scalarArgB = sink(one(7), 9)
local anonymous = function()
    -- A comment does not add a statement to the body.
end
anonymous()
local object = {}
function object:empty() end
object:empty()
local observedFirst = first
local observedSecond = second
local observedScalar = scalar
local observedExisting = existing
local observedTailFirst = tailFirst
local observedTailSecond = tailSecond
local observedTailArgA = tailArgA
local observedTailArgB = tailArgB
local observedScalarArgA = scalarArgA
local observedScalarArgB = scalarArgB
