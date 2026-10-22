local alias = Use
local savedSource = Source
Use = function(value) return "local" end
alias(savedSource())
Use(savedSource())
do
    local Source = function() return "shadow" end
    alias(Source())
end
local saved = Source
Source = function() return "replaced" end
alias(saved())
alias(Source())
