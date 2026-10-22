local function Mark(value) return value end
local function Limited()
    for i = 1, 65 do
        Mark(i)
    end
    Mark("after_function_loop")
end
local stopped = Limited()
Mark("after_stopped_call")
