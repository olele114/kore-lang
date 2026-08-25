	.file	"kore_module"
	.text
	.globl	test_some                       // -- Begin function test_some
	.p2align	2
	.type	test_some,@function
test_some:                              // @test_some
	.cfi_startproc
// %bb.0:                               // %entry
	sub	sp, sp, #16
	.cfi_def_cfa_offset 16
	mov	x8, #429496729600               // =0x6400000000
	stp	x8, x8, [sp], #16
	mov	w0, #1                          // =0x1
	ret
.Lfunc_end0:
	.size	test_some, .Lfunc_end0-test_some
	.cfi_endproc
                                        // -- End function
	.globl	test_none                       // -- Begin function test_none
	.p2align	2
	.type	test_none,@function
test_none:                              // @test_none
	.cfi_startproc
// %bb.0:                               // %entry
	sub	sp, sp, #16
	.cfi_def_cfa_offset 16
	strb	wzr, [sp, #4]
	mov	w8, #1                          // =0x1
	mov	w0, #2                          // =0x2
	ldr	w9, [sp, #4]
	str	w8, [sp]
	stp	w8, w9, [sp, #8]
	add	sp, sp, #16
	ret
.Lfunc_end1:
	.size	test_none, .Lfunc_end1-test_none
	.cfi_endproc
                                        // -- End function
	.globl	main                            // -- Begin function main
	.p2align	2
	.type	main,@function
main:                                   // @main
	.cfi_startproc
// %bb.0:                               // %entry
	sub	sp, sp, #48
	str	x30, [sp, #32]                  // 8-byte Folded Spill
	.cfi_def_cfa_offset 48
	.cfi_offset w30, -16
	bl	test_some
	stp	w0, w0, [sp, #40]
	bl	test_none
	ldr	w8, [sp, #44]
	ldr	x30, [sp, #32]                  // 8-byte Folded Reload
	stp	w0, w0, [sp, #24]
	stp	w0, w8, [sp, #16]
	add	w8, w8, w0
	mov	w0, w8
	str	w8, [sp, #12]
	add	sp, sp, #48
	ret
.Lfunc_end2:
	.size	main, .Lfunc_end2-main
	.cfi_endproc
                                        // -- End function
	.section	".note.GNU-stack","",@progbits
