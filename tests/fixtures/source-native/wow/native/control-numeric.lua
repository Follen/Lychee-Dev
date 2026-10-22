local function Mark(value) return value end
for i = 1, 2 do
    local result = Mark(i)
end
for i = 3, 1, -1 do
    local descending = Mark(i)
end
for i = 2, 1 do
    Mark("zero_iterations")
end
local previous
local current
for i = 1, 2 do
    previous = current
    local captured = i
    current = function() return captured end
end
local first = previous()
local second = current()
