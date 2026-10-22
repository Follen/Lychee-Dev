local function Mark(value) return value end
while true do
    Mark("reachable")
end
Mark("after_nontermination")
