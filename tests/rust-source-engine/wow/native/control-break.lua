local function Mark(value) return value end
for i = 1, 2 do
    Mark(i)
    break
end
Mark("after_break")
