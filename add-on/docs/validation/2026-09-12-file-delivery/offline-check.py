"""Offline Lua 5.1 envelope experiment; never accesses a game installation."""
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
import zlib


def lua_string(value):
    return '"' + ''.join(chr(b) if 32 <= b < 127 and b not in (34, 92)
                         else '\\%03d' % b for b in value) + '"'


HARNESS = r'''
local taskPath, sourcePath, expectedLength, expectedSum, mode = ...
local function checksum(s)
    local a, b = 1, 0
    for i = 1, #s do a = (a + s:byte(i)) % 65521; b = (b + a) % 65521 end
    return b * 65536 + a
end
local staged
local ns = { DevTasks = { Stage = function(t) staged = t end } }
local loader = assert(loadfile(taskPath))
loader("OfflineFixture", ns)
assert(staged and staged.id == "offline-only")
assert(_G.OFFLINE_EXECUTED == nil, "loading ran the payload")
local f = assert(io.open(sourcePath, "rb")); local raw = f:read("*a"); f:close()
assert(staged.source == raw, "payload byte roundtrip failed")
assert(#staged.source == tonumber(expectedLength))
assert(checksum(staged.source) == tonumber(expectedSum))
assert(staged.checksum == tonumber(expectedSum))
local damaged = staged.source .. " "
assert(#damaged ~= tonumber(expectedLength) and checksum(damaged) ~= staged.checksum)
if mode == "bytes" then
    print("PASS exact bytes, no execution, corruption detectable")
elseif mode == "syntax" then
    local fn, err = loadstring(staged.source, "OfflineTask")
    assert(fn == nil and type(err) == "string")
    assert(_G.OFFLINE_EXECUTED == nil)
    print("PASS invalid source staged without execution; explicit compile rejected")
else
    local fn = assert(loadstring(staged.source, "OfflineTask"))
    assert(_G.OFFLINE_EXECUTED == nil, "compile ran the payload")
    local ok, result = pcall(fn, { value = 7 })
    assert(ok and result == 49 and _G.OFFLINE_EXECUTED == 1)
    print("PASS large source staged and compiled without execution; explicit call returned 49")
end
'''


def main():
    body = b'local params = ...; _G.OFFLINE_EXECUTED = 1; return params.value * params.value\n'
    source = b'--' + b'x' * (468817 - len(body) - 3) + b'\n' + body
    assert len(source) == 468817
    cases = [('large-source', source, 'execute'),
             ('all-byte-values', bytes(range(256)) + b'\r\n]] ]=] "\\000', 'bytes'),
             ('invalid-syntax', b'local = broken', 'syntax')]
    results = []
    with tempfile.TemporaryDirectory(prefix='lychee-file-envelope-') as temporary:
        root = Path(temporary)
        harness = root / 'harness.lua'
        harness.write_text(HARNESS, encoding='ascii')
        for name, payload, mode in cases:
            source_file = root / 'source.bin'
            source_file.write_bytes(payload)
            checksum = zlib.adler32(payload) & 0xffffffff
            generated = ('local ADDON_NAME, ns = ...\n'
                         'ns.DevTasks.Stage({id="offline-only", checksum=%d, source=%s})\n'
                         % (checksum, lua_string(payload)))
            # Temp file and replace model complete-file publication, not WoW reload.
            pending, task = root / 'Task.lua.tmp', root / 'Task.lua'
            pending.write_bytes(generated.encode('ascii'))
            pending.replace(task)
            completed = subprocess.run(['lua', str(harness), str(task), str(source_file),
                                        str(len(payload)), str(checksum), mode],
                                       capture_output=True, text=True, timeout=10, check=True)
            results.append(dict(case=name, sourceBytes=len(payload),
                                envelopeBytes=task.stat().st_size,
                                sha256=hashlib.sha256(payload).hexdigest(),
                                output=completed.stdout.strip()))
    print(json.dumps(dict(scope='Offline Lua only; no WoW, input, SV, or runtime protocol test',
                          cases=results), indent=2))


if __name__ == '__main__':
    main()
