local value = 1
local function outer()
    local captured = value
    do
        local value = 2
        local inner_read = value
    end
    return value, captured
end
return value, outer
