C_CVar.RegisterCVar("graphicsQuality", "5", "Controls the overall graphics quality preset.")
SetCVar("Sound_EnableSFX", "0")
if GetCVarBool("Sound_EnableSFX") then
	print("sound effects enabled")
end
