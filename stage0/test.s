	.file	"kore_module"
	.text
	.globl	add
	.p2align	2
	.type	add,@function
add:
	.cfi_startproc
	sub	sp, sp, #32
	.cfi_def_cfa_offset 32
	ldp	w9, w8, [sp, #24]
	add	w0, w8, w9
	stp	w9, w8, [sp, #16]
	str	w0, [sp, #12]
	add	sp, sp, #32
	ret
.Lfunc_end0:
	.size	add, .Lfunc_end0-add
	.cfi_endproc

	.globl	main
	.p2align	2
	.type	main,@function
main:
	.cfi_startproc
	str	x30, [sp, #-16]!
	.cfi_def_cfa_offset 16
	.cfi_offset w30, -16
	mov	w0, #10
	mov	w1, #32
	bl	add
	str	w0, [sp, #12]
	ldr	x30, [sp], #16
	ret
.Lfunc_end1:
	.size	main, .Lfunc_end1-main
	.cfi_endproc

	.section	".note.GNU-stack","",@progbits
