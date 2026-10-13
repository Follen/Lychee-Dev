local banner = "中文😀"; local target = 42
---@type number
local bad = "wrong"
local alias = target
local function caller()
    return target
end
caller()
return alias, bad
