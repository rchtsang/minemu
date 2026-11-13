/* Minimal user module: the baseline kernel only checks that it is packaged. */
	.text
	.global __minemu_user_start
__minemu_user_start:
	b	__minemu_user_start
