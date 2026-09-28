#include "textflag.h"

// Return the first two-anchor candidate among complete groups of 32 starts.
// The farthest load ends at start+distance+31, strictly inside data.
TEXT ·anchorAVX2(SB), NOSPLIT, $0-48
    MOVQ data_base+0(FP), SI
    MOVQ data_len+8(FP), CX
    MOVQ distance+32(FP), R8
    SUBQ R8, CX
    MOVBLZX first+24(FP), AX
    MOVD AX, X0
    VPBROADCASTB X0, Y0
    MOVBLZX last+25(FP), AX
    MOVD AX, X1
    VPBROADCASTB X1, Y1
    XORQ R9, R9
loop:
    CMPQ CX, $32
    JL missing
    VMOVDQU (SI), Y2
    VPCMPEQB Y0, Y2, Y2
    VMOVDQU (SI)(R8*1), Y3
    VPCMPEQB Y1, Y3, Y3
    VPAND Y2, Y3, Y2
    VPMOVMSKB Y2, AX
    TESTL AX, AX
    JNZ found
    ADDQ $32, SI
    ADDQ $32, R9
    SUBQ $32, CX
    JMP loop
found:
    BSFL AX, AX
    ADDQ R9, AX
    MOVQ AX, ret+40(FP)
    VZEROUPPER
    RET
missing:
    MOVQ $-1, ret+40(FP)
    VZEROUPPER
    RET
