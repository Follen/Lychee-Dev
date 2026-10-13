local a = {}
local b = {}
local old = a
a, a.run = b, F1
local oldCall = old.run(7)
local newRead = a.run
return oldCall, newRead
