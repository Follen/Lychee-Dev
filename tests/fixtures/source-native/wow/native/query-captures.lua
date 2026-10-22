local value = 1
local function Read() return value end
local first = Read()
value = "later"
local second = Read()
local function Factory(seed)
    local value = seed
    return function() return value end
end
local one = Factory(1)
local two = Factory("two")
local firstFactoryCall = one()
local secondFactoryCall = two()
