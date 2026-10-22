---@generic T
---@param value T
---@return T
local function Identity(value)
    local copy = value
    return copy
end
local first = Identity(1)
local second = Identity("two")
local observedFirst = first
local observedSecond = second
