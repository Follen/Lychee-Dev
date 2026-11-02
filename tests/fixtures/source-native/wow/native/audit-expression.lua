local o = { value = 1 }
local function Mutate(x)
    x.value = "changed"
    return true
end
local result = Mutate(o) or false
local after = o.value
