function Box:Wrap(value) return value end
local x = Box:Wrap(Source())
Use(x)
local viaDot = Box.Wrap(Box, Pair())
Use(viaDot)
