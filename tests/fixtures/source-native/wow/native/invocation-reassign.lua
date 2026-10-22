local function Reassign(x)
    x = { value = "new" }
    return x
end
local original = { value = 1 }
local rebound = Reassign(original)
local afterOriginal = original.value
local afterRebound = rebound.value
local Methods = { value = 3 }
function Methods:Self()
    return self
end
local same = Methods:Self()
local afterSelf = same.value
