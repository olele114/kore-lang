; ModuleID = 'kore_module'
source_filename = "kore_module"

declare void @print({ ptr, i64 })

declare void @println({ ptr, i64 })

declare { ptr, i64 } @read_file({ ptr, i64 })

declare i32 @write_file({ ptr, i64 }, { ptr, i64 })

declare void @eprint({ ptr, i64 })

declare void @eprintln({ ptr, i64 })

define i32 @kore_main() {
entry:
  br label %bb0

bb0:                                              ; preds = %entry
  ret i32 42
}

declare void @kore_init_cmdline_args(i32, ptr)

define i32 @main(i32 %0, ptr %1) {
entry:
  call void @kore_init_cmdline_args(i32 %0, ptr %1)
  %result = call i32 @kore_main()
  ret i32 %result
}
