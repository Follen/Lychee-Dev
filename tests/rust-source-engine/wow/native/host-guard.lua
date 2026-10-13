function Guarded()
    local x = Source()
    if issecretvalue(x) then return end
    Use(x)
end
