local function Mark(value) return value end
repeat
    local done = true
    Mark("once")
until done
repeat
    Mark("break")
    break
until false
Mark("after")
