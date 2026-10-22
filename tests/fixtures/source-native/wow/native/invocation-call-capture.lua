local x = 1
local function f()
    return x
end
local first = f()
x = "s"
local second = f()
local firstRead = first
local secondRead = second
