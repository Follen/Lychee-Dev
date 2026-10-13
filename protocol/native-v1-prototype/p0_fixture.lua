local root=assert(arg[1])
local base=root..'/protocol/native-v1-prototype/'
local crypto=assert(loadfile(base..'crypto.lua'))()
local receiver=assert(loadfile(base..'observation.lua'))()
local key=string.rep(string.char(0x19),32) -- Public test key, not a production secret.
local binding,runtime,session=string.rep('1',32),string.rep('2',32),string.rep('3',32)
local digest=crypto.sha256('RUN|return 7|budget=5|policy=opaque')
local function emit(label,value) io.write(label,'\t',value,'\n'); io.flush() end
local abc=crypto.sha256('abc')
assert(abc=='ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad','sha256_abc:'..abc)
assert(crypto.hmac(string.rep(string.char(0x0b),20),'Hi There')=='b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7')
assert(crypto.hmac('Jefe','what do ya want for nothing?')=='5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843')
assert(crypto.hmac(string.rep(string.char(0xaa),131),'Test Using Larger Than Block-Size Key - Hash Key First')=='60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54')
emit('VECTORS','sha256-abc+rfc4231-1,2,6')
local peer=receiver.new(crypto,key,binding,runtime,session)
peer.controller.set(false,false,false,10250)
local a=peer.stage(1,string.rep('a',64),digest)
peer.controller.set(false,false,false,10500)
local b=peer.controller.republish()
-- Attacker does not receive peer/controller/key/crypto. It can only copy saved
-- public strings, while the still-running receiver has private combat=true.
local function public_writer(box,saved) for i=1,#saved do box.STATE=saved[i]; emit('OLD'..i,box.STATE) end end
peer.controller.set(true,false,false,99000)
public_writer(peer.public,{a,b})
emit('DIGEST',digest)
emit('PRIVATE_BLOCKED','true')
local nonce=assert(io.read('*l'),'host_nonce_required')
emit('CURRENT_BLOCKED',peer.stage(2,nonce,digest))
peer.controller.set(false,false,false,99250)
emit('CURRENT_READY',peer.controller.republish())
-- Signing work benchmark is diagnostic only, not a client performance gate.
local start=os.clock()
for _=1,10 do crypto.hmac(key,string.rep('s',8192)) end
emit('LUA_8K_HMAC_10_CPU_SECONDS',string.format('%.6f',os.clock()-start))
emit('SCOPE','real_lua51_public_replay;not_wow_or_os_input;no_business_executed')
