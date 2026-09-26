local root=assert(arg[1])
local ns={}
for _,name in ipairs({'CaptureWriter','MatrixSymbol'}) do assert(loadfile(root..'/Bridge/'..name..'.lua'))('Lychee Dev',ns) end
local nonce='26161b3b1bd87b850000000000000001'
local report=assert(ns.CaptureWriter.EncodeSignal({schema='lycheedev.signal.v1',release='2.0.2',kind='reported',
 sessionNonce=nonce,requestId='REQ-3b1f0a14435c866f43abcff4a2f5bbcd',product='classic',build='5.5.4.69934',
 sequence=8,inputReady=false,codeBytes=202,codeAdler32='00b941f2',reportBytes=101,reportAdler32='faf021a6'}))
local ready,_,signal=ns.CaptureWriter.EncodeSignal({schema='lycheedev.signal.v1',release='2.0.2',kind='ready',
 sessionNonce=nonce,requestId='',product='classic',build='5.5.4.69934',sequence=9,runtimeEpoch=2,inputReady=true})
local pair=assert(ns.CaptureWriter.EncodeReceiptPair(report,signal))
local a,b,c=#assert(ns.MatrixSymbol.Encode(report)),#assert(ns.MatrixSymbol.Encode(ready)),#assert(ns.MatrixSymbol.Encode(pair))
local oldWidth,oldHeight=(math.max(a,b)+8)*3,(a+b+16)*3
local side=(c+8)*3
assert(side*side<oldWidth*oldHeight,'single optical symbol did not reduce area')
local goodSequence=signal.sequence
signal.inputReady=false
assert(ns.CaptureWriter.EncodeReceiptPair(report,signal)==nil,'unready state acquired input permission')
signal.inputReady=true
local secret={}
issecretvalue=function(v) return rawequal(v,secret) end
signal.sequence=secret
assert(ns.CaptureWriter.EncodeReceiptPair(report,signal)==nil,'secret sequence encoded')
signal.sequence=goodSequence
io.write(assert(ns.CaptureWriter.Encode({receipt=report,ready=ready,pair=pair,oldWidth=oldWidth,oldHeight=oldHeight,newSide=side})))
