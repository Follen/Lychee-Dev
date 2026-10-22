local function Mark(value) return value end
local function Unknown(iterator)
    for key, value in iterator do
        Mark(value)
    end
    Mark("after_function_iterator")
end
local stopped = Unknown(...)
Mark("after_stopped_call")
