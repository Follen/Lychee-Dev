local function Mark(value) return value end
local function Early(flag)
    if flag then
        return Mark("early")
    end
    Mark("continued")
    return "later"
end
local function LoopReturn()
    for i = 1, 2 do
        return Mark(i)
    end
    Mark("unreachable")
end
local yes = Early(true)
local no = Early(false)
local unknown = ...
local either = Early(unknown)
local returned = LoopReturn()
