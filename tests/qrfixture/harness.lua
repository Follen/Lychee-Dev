-- Offline harness for the real add-on QR encoder.
--
-- The first argument is the path to AutomationQR.lua. The payload is read from
-- stdin so the process boundary preserves the fixture's UTF-8 bytes.

local encoder_path = arg[1]
if not encoder_path then
    io.stderr:write("missing AutomationQR.lua path\n")
    os.exit(2)
end

local payload = io.read("*a")
if type(payload) ~= "string" or payload == "" then
    io.stderr:write("empty payload\n")
    os.exit(2)
end

local chunk, load_error = loadfile(encoder_path)
if not chunk then
    io.stderr:write("load AutomationQR.lua: " .. tostring(load_error) .. "\n")
    os.exit(2)
end

local namespace = {}
local loaded, run_error = pcall(chunk, "Lychee Dev", namespace)
if not loaded then
    io.stderr:write("run AutomationQR.lua: " .. tostring(run_error) .. "\n")
    os.exit(2)
end

-- The 2.0 addon ships MatrixSymbol.lua (same luaqrcode lineage, fixed M
-- correction); the retired 1.x Libs/AutomationQR.lua remains accepted so the
-- oracle still covers the historical encoder if it is present.
if not (namespace.MatrixSymbol and type(namespace.MatrixSymbol.Encode) == "function")
    and not (namespace.AutomationQR and type(namespace.AutomationQR.Encode) == "function") then
    io.stderr:write("QR encoder module is unavailable\n")
    os.exit(2)
end

local encoder = namespace.MatrixSymbol or namespace.AutomationQR
local matrix, encode_error = encoder.Encode(payload)
if not matrix then
    io.stderr:write("AutomationQR.Encode: " .. tostring(encode_error) .. "\n")
    os.exit(2)
end

local width = #matrix
if width < 21 then
    io.stderr:write("unexpected matrix width: " .. tostring(width) .. "\n")
    os.exit(2)
end

for x = 1, width do
    if type(matrix[x]) ~= "table" or #matrix[x] ~= width then
        io.stderr:write("matrix is not square\n")
        os.exit(2)
    end
end

io.write("LYCHEE_QR_MATRIX 1 ", width, "\n")
for y = 1, width do
    for x = 1, width do
        local value = matrix[x][y]
        if type(value) ~= "number" then
            io.stderr:write("matrix contains a non-number value\n")
            os.exit(2)
        end
        io.write(value > 0 and "1" or "0")
    end
    io.write("\n")
end
io.write("END\n")
