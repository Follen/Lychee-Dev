local function Make(seed)
    local value = seed
    return function(newValue)
        if newValue then
            value = newValue
        end
        return value
    end
end
local first = Make(1)
local second = Make("two")
local firstBefore = first(nil)
local secondBefore = second(nil)
local firstAfter = first("one")
local secondAfter = second(nil)
local observedFirstBefore = firstBefore
local observedSecondBefore = secondBefore
local observedFirstAfter = firstAfter
local observedSecondAfter = secondAfter
