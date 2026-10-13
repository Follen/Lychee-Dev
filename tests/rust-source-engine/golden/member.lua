local TableProbe = {}
function TableProbe:run() return 1 end
local value = TableProbe.run
TableProbe:run()
return value
