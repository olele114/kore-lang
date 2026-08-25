; ModuleID = 'kore_module'
source_filename = "kore_module"

declare void @print({ ptr, i64 })

declare void @println({ ptr, i64 })

declare { ptr, i64 } @read_file({ ptr, i64 })

declare i32 @write_file({ ptr, i64 }, { ptr, i64 })

define i32 @clamp(i32 %0) {
entry:
  %_x_0 = alloca i32, align 4
  %_tmp_1 = alloca i1, align 1
  %_tmp_2 = alloca i32, align 4
  %_tmp_3 = alloca i1, align 1
  %_tmp_4 = alloca i32, align 4
  %_tmp_5 = alloca i1, align 1
  store i32 %0, ptr %_x_0, align 4
  br label %bb0

bb0:                                              ; preds = %entry
  store i1 true, ptr %_tmp_1, align 1
  unreachable

bb2:                                              ; No predecessors!

bb3:                                              ; No predecessors!

bb4:                                              ; No predecessors!
  %load6 = load i1, ptr %_tmp_3, align 1
  switch i1 %load6, label %bb5 [
    i1 true, label %bb21
  ]

bb5:                                              ; preds = %bb4
  %load7 = load i1, ptr %_tmp_5, align 1
  switch i1 %load7, label %bb32 [
    i1 true, label %bb32
  ]

bb21:                                             ; preds = %bb4
  ret i32 0
  unreachable

bb32:                                             ; preds = %bb5, %bb5
  %load = load i32, ptr %_x_0, align 4
  store i32 %load, ptr %_tmp_2, align 4
  %load3 = load i32, ptr %_tmp_2, align 4
  %lt = icmp slt i32 %load3, 0
  store i1 %lt, ptr %_tmp_3, align 1
  %load4 = load i32, ptr %_x_0, align 4
  store i32 %load4, ptr %_tmp_4, align 4
  %load5 = load i32, ptr %_tmp_4, align 4
  %gt = icmp sgt i32 %load5, 100
  store i1 %gt, ptr %_tmp_5, align 1
  ret i32 100
  unreachable
}

define i32 @main() {
entry:
  %_tmp_0 = alloca i32, align 4
  br label %bb0

bb0:                                              ; preds = %entry
  %call = call i32 @clamp(i32 42)
  store i32 %call, ptr %_tmp_0, align 4
  %load = load i32, ptr %_tmp_0, align 4
  ret i32 %load
}
