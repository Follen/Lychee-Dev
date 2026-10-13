local t = {}
t.run = F1
local function mutate(o)
    o.run = F2
end
mutate(t)
local after = t.run(7)
return after
