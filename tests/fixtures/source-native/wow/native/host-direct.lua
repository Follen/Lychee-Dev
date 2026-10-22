local sourceAlias = Source
local safe, secret = Pair()
local x = sourceAlias()
Use(x)
UseSecond(Pair())
Allow(x)
local guard = issecretvalue(x)
