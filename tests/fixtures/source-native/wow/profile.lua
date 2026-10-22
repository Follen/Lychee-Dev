-- Fixture profile declarations. Their exact declaration IDs authorize the
-- explicitly assumed shallow-copy contract; these bodies are never executed.
function Mixin(target, source) end
function CreateFromMixins(source) end
function setmetatable(target, meta) end
