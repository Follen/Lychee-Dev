local probe=...
assert(not probe:IsInputProtected())
assert(not LycheeDevInternal.InputProtection.IsActive())
return {released=true}
