; ModuleID = 'kore_module'
source_filename = "kore_module"

declare void @print({ ptr, i64 })

declare void @println({ ptr, i64 })

declare { ptr, i64 } @read_file({ ptr, i64 })

declare i32 @write_file({ ptr, i64 }, { ptr, i64 })

define i32 @test_some() {
entry:
  %_x_0 = alloca { i32, i32 }, align 8
  %_tmp_1 = alloca { i32, i32 }, align 8
  br label %bb0

bb0:                                              ; preds = %entry
  store { i32, i32 } { i32 0, i32 100 }, ptr %_tmp_1, align 4
  %load = load { i32, i32 }, ptr %_tmp_1, align 4
  store { i32, i32 } %load, ptr %_x_0, align 4
  ret i32 1
}

define i32 @test_none() {
entry:
  %_y_0 = alloca { i32, i32 }, align 8
  %_tmp_1 = alloca { i32, i32 }, align 8
  br label %bb0

bb0:                                              ; preds = %entry
  store { i32, i32 } { i32 1, i32 0 }, ptr %_tmp_1, align 4
  %load = load { i32, i32 }, ptr %_tmp_1, align 4
  store { i32, i32 } %load, ptr %_y_0, align 4
  ret i32 2
}

define i32 @main() {
entry:
  %_a_0 = alloca i32, align 4
  %_tmp_1 = alloca i32, align 4
  %_b_2 = alloca i32, align 4
  %_tmp_3 = alloca i32, align 4
  %_tmp_4 = alloca i32, align 4
  %_tmp_5 = alloca i32, align 4
  %_tmp_6 = alloca i32, align 4
  br label %bb0

bb0:                                              ; preds = %entry
  %call = call i32 @test_some()
  store i32 %call, ptr %_tmp_1, align 4
  %load = load i32, ptr %_tmp_1, align 4
  store i32 %load, ptr %_a_0, align 4
  %call1 = call i32 @test_none()
  store i32 %call1, ptr %_tmp_3, align 4
  %load2 = load i32, ptr %_tmp_3, align 4
  store i32 %load2, ptr %_b_2, align 4
  %load3 = load i32, ptr %_a_0, align 4
  store i32 %load3, ptr %_tmp_4, align 4
  %load4 = load i32, ptr %_b_2, align 4
  store i32 %load4, ptr %_tmp_5, align 4
  %load5 = load i32, ptr %_tmp_4, align 4
  %load6 = load i32, ptr %_tmp_5, align 4
  %add = add i32 %load5, %load6
  store i32 %add, ptr %_tmp_6, align 4
  %load7 = load i32, ptr %_tmp_6, align 4
  ret i32 %load7
}
