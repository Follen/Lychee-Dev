local source = assert(arg[1])
local ns = {}
assert(loadfile(source))("Lychee Dev", ns)
local writer = ns.CaptureWriter
local code = "return { answer = 42 }"
local body = assert(writer.Encode({ answer = 42, text = "\228\184\150\231\149\140", complete = true }))
local receipt = assert(writer.Encode({
    schema = "lycheedev.signal.v1", release = "2.0.6", kind = "reported",
    sessionNonce = "session", requestId = "OP-protocol", character = "character",
    realm = "realm", product = "retail", build = "12.1.0.69875", sequence = 4,
    inputReady = false, codeBytes = #code, codeAdler32 = assert(writer.DigestBytes(code)),
    reportBytes = #body, reportAdler32 = assert(writer.DigestBytes(body)),
}, 4096))
-- One outer JSON frame preserves each embedded document's exact UTF-8 bytes.
io.write(assert(writer.Encode({ receipt = receipt, body = body, code = code })))
