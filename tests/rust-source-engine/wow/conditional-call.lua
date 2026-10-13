local Flag = ...
local t = {}
t.run = F1
local source = {}
source.run = F2
if Flag then
    Mixin(t, source)
end
local after = t.run(7)
return after
