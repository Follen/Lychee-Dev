local value = 10
for value = value, value + 2 do
    local numericBody = value
end
local numericAfter = value
for value in Iterator(value) do
    local genericBody = value
end
local genericAfter = value
for value = 1, 2 do
    for value = value, value + 1 do
        local nestedBody = value
    end
    local afterInner = value
end
local nestedAfter = value
local function Recursive(n)
    if n then return Recursive(false) end
    return value
end
