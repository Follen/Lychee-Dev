local function Mark(value) return value end
local iterator = ...
for key, value in iterator do
    Mark(value)
end
Mark("after_unknown_iterator")
