local function Empty()
    return
end
local function Pair()
    return 1, "two"
end
local function Forward()
    return Pair()
end
local function Parenthesized()
    return (Pair())
end
---@return string
local function Annotated()
    return 1
end
local scalarZero = Empty()
local emptyA, emptyB = Empty()
local scalarPair = Pair()
local pairA, pairB = Pair()
local prefixA, prefixB = Pair(), 9
local parenA, parenB = (Pair())
local forwardA, forwardB = Forward()
local singleA, singleB = Parenthesized()
local contractValue = Annotated()
local readEmptyB = emptyB
local readPairB = pairB
local readParenB = parenB
local readForwardB = forwardB
local readSingleB = singleB
