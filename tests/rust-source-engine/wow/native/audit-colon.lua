local o = {
    value = 1,
    mutate = function(self)
        self.value = "changed"
    end,
}
o:mutate()
local after = o.value
