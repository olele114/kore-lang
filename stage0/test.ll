; ModuleID = 'kore_module'
source_filename = "kore_module"

define i32 @add(i32 %0, i32 %1) {
entry:
  %a_0 = alloca i32, align 4
  %b_1 = alloca i32, align 4
  %tmp_2 = alloca i32, align 4
  %tmp_3 = alloca i32, align 4
  %tmp_4 = alloca i32, align 4
  br label %bb0

bb0:                                              ; preds = %entry
  %load = load i32, ptr %a_0, align 4
  store i32 %load, ptr %tmp_2, align 4
  %load1 = load i32, ptr %b_1, align 4
  store i32 %load1, ptr %tmp_3, align 4
  %load2 = load i32, ptr %tmp_2, align 4
  %load3 = load i32, ptr %tmp_3, align 4
  %add = add i32 %load2, %load3
  store i32 %add, ptr %tmp_4, align 4
  %load4 = load i32, ptr %tmp_4, align 4
  ret i32 %load4
}

define i32 @main() {
entry:
  %tmp_0 = alloca i32, align 4
  br label %bb0

bb0:                                              ; preds = %entry
  %call = call i32 @add(i32 10, i32 32)
  store i32 %call, ptr %tmp_0, align 4
  %load = load i32, ptr %tmp_0, align 4
  ret i32 %load
}
