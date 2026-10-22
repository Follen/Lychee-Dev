---@param x { value: number }|nil
---@return number
local function NeedUnion(x)
    return 0
end
---@param x { value: number }?
---@return number
local function NeedNullable(x)
    return 0
end
local bad = { value = "wrong" }
local good = { value = 1 }
local unionBad = NeedUnion(bad)
local unionGood = NeedUnion(good)
local unionNil = NeedUnion(nil)
local nullableBad = NeedNullable(bad)
local nullableGood = NeedNullable(good)
local nullableNil = NeedNullable(nil)
