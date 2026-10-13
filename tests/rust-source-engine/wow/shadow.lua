---@param left number
---@param right number
---@return number
local function Mixin(left, right)
    return left
end
local target = {}
local source = {}
source.run = F1
local plain = Mixin(1, 2)
local wrong = Mixin(target, source)
local absent = target.run
return plain, absent
