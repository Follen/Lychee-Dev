local hidden = { value = 1 }
local untouched = { value = 7 }
local unknown = ...
unknown(function()
    return hidden
end)
hidden = { value = "fresh" }
unknown()
local escapedCurrent = hidden.value
local untouchedCurrent = untouched.value
