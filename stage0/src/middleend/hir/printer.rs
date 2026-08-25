//! HIR 调试打印器。
//!
//! 类似 rustc -Z dump-mir，将 HIR 格式化为人类可读的文本，用于调试降级过程。

use super::*;
use std::fmt::Write;

/// HIR 打印器配置
pub struct PrinterConfig {
    /// 是否显示 Span 信息
    pub show_spans: bool,
    /// 缩进宽度
    pub indent: usize,
}

impl Default for PrinterConfig {
    fn default() -> Self {
        Self {
            show_spans: false,
            indent: 2,
        }
    }
}

/// HIR 打印器
pub struct Printer {
    config: PrinterConfig,
    output: String,
    indent_level: usize,
}

impl Printer {
    pub fn new(config: PrinterConfig) -> Self {
        Self {
            config,
            output: String::new(),
            indent_level: 0,
        }
    }

    pub fn with_default() -> Self {
        Self::new(PrinterConfig::default())
    }

    /// 获取打印结果
    pub fn finish(self) -> String {
        self.output
    }

    // ────────────────────────────────────────────────────────────────────────
    // 辅助方法
    // ────────────────────────────────────────────────────────────────────────

    fn write(&mut self, s: &str) {
        self.output.push_str(s);
    }

    fn writeln(&mut self, s: &str) {
        self.write(s);
        self.output.push('\n');
    }

    fn indent(&mut self) {
        for _ in 0..(self.indent_level * self.config.indent) {
            self.output.push(' ');
        }
    }

    fn write_indented(&mut self, s: &str) {
        self.indent();
        self.write(s);
    }

    fn writeln_indented(&mut self, s: &str) {
        self.indent();
        self.writeln(s);
    }

    fn inc_indent(&mut self) {
        self.indent_level += 1;
    }

    fn dec_indent(&mut self) {
        self.indent_level = self.indent_level.saturating_sub(1);
    }

    fn write_span(&mut self, span: &Span) {
        if self.config.show_spans {
            write!(&mut self.output, " // {:?}", span).unwrap();
        }
    }

    // ────────────────────────────────────────────────────────────────────────
    // 模块级别打印
    // ────────────────────────────────────────────────────────────────────────

    /// 打印整个模块
    pub fn print_module(&mut self, module: &HirModule) {
        self.writeln("// HIR Module");
        self.writeln("");

        // 打印全局变量
        if !module.globals.is_empty() {
            self.writeln("// Globals");
            for global in &module.globals {
                self.print_global(global);
            }
            self.writeln("");
        }

        // 打印结构体定义
        if !module.structs.is_empty() {
            self.writeln("// Structs");
            for struc in &module.structs {
                self.print_struct(struc);
            }
            self.writeln("");
        }

        // 打印联合体定义
        if !module.unions.is_empty() {
            self.writeln("// Unions");
            for union in &module.unions {
                self.print_union(union);
            }
            self.writeln("");
        }

        // 打印函数
        for (i, func) in module.functions.iter().enumerate() {
            if i > 0 {
                self.writeln("");
            }
            self.print_function(func);
        }
    }

    fn print_global(&mut self, global: &HirGlobal) {
        write!(&mut self.output, "global {} : {}", global.name, global.ty).unwrap();
        if let Some(init) = &global.init {
            write!(&mut self.output, " = {:?}", init).unwrap();
        }
        self.write_span(&global.span);
        self.writeln("");
    }

    fn print_struct(&mut self, struc: &HirStruct) {
        write!(&mut self.output, "struct {}", struc.name).unwrap();
        self.write_span(&struc.span);
        self.writeln(" {");
        self.inc_indent();
        for field in &struc.fields {
            self.write_indented(&format!("{}: {}", field.name, field.ty));
            self.write_span(&field.span);
            self.writeln("");
        }
        self.dec_indent();
        self.writeln("}");
    }

    fn print_union(&mut self, union: &HirUnion) {
        write!(&mut self.output, "union {}", union.name).unwrap();
        self.write_span(&union.span);
        self.writeln(" {");
        self.inc_indent();
        for variant in &union.variants {
            self.write_indented(&format!("{}", variant.name));
            if let Some(payload) = &variant.payload {
                write!(&mut self.output, "({})", payload).unwrap();
            }
            self.write_span(&variant.span);
            self.writeln("");
        }
        self.dec_indent();
        self.writeln("}");
    }

    fn print_function(&mut self, func: &HirFunction) {
        write!(&mut self.output, "fn {}(", func.name).unwrap();
        for (i, param) in func.params.iter().enumerate() {
            if i > 0 {
                self.write(", ");
            }
            write!(&mut self.output, "{}: {}", param.name, param.ty).unwrap();
        }
        write!(&mut self.output, ") -> {}", func.ret_type).unwrap();
        self.write_span(&func.span);

        if let Some(body) = &func.body {
            self.writeln(" {");
            self.inc_indent();
            self.print_body(body);
            self.dec_indent();
            self.writeln("}");
        } else {
            self.writeln(";  // builtin");
        }
    }

    fn print_body(&mut self, body: &HirBody) {
        // 打印局部变量
        if !body.locals.is_empty() {
            self.writeln_indented("// Locals");
            for (i, local) in body.locals.iter().enumerate() {
                self.write_indented(&format!("let _{}: {}", i, local.ty));
                if let Some(name) = &local.name {
                    write!(&mut self.output, "  // {}", name).unwrap();
                }
                self.write_span(&local.span);
                self.writeln("");
            }
            self.writeln("");
        }

        // 打印所有基本块
        for block in &body.blocks {
            self.print_block(block);
            self.writeln("");
        }
    }

    fn print_block(&mut self, block: &HirBlock) {
        self.write_indented(&format!("bb{}", block.id.0));
        self.write_span(&block.span);
        self.writeln(":");
        self.inc_indent();

        // 打印语句
        for stmt in &block.stmts {
            self.print_stmt(stmt);
        }

        // 打印终结符
        self.print_terminator(&block.terminator);

        self.dec_indent();
    }

    fn print_stmt(&mut self, stmt: &HirStmt) {
        match stmt {
            HirStmt::Assign { lhs, rhs, span } => {
                self.write_indented("");
                self.print_place(lhs);
                self.write(" = ");
                self.print_rvalue(rhs);
                self.write_span(span);
                self.writeln("");
            }
            HirStmt::Call { dest, func, args, span } => {
                self.write_indented("");
                if let Some(place) = dest {
                    self.print_place(place);
                    self.write(" = ");
                }
                self.print_operand(func);
                self.write("(");
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.print_operand(arg);
                }
                self.write(")");
                self.write_span(span);
                self.writeln("");
            }
            HirStmt::Drop { place, span } => {
                self.write_indented("drop(");
                self.print_place(place);
                self.write(")");
                self.write_span(span);
                self.writeln("");
            }
        }
    }

    fn print_terminator(&mut self, term: &HirTerminator) {
        match term {
            HirTerminator::Return(op) => {
                self.write_indented("return");
                if let Some(operand) = op {
                    self.write(" ");
                    self.print_operand(operand);
                }
                self.writeln("");
            }
            HirTerminator::Goto(block) => {
                self.write_indented(&format!("goto bb{}", block.0));
                self.writeln("");
            }
            HirTerminator::Switch { discr, targets, otherwise } => {
                self.write_indented("switch ");
                self.print_operand(discr);
                self.writeln(" {");
                self.inc_indent();
                for (value, target) in targets {
                    self.writeln_indented(&format!("{} => bb{}", value, target.0));
                }
                self.writeln_indented(&format!("_ => bb{}", otherwise.0));
                self.dec_indent();
                self.writeln_indented("}");
            }
            HirTerminator::Unreachable => {
                self.writeln_indented("unreachable");
            }
        }
    }

    fn print_rvalue(&mut self, rvalue: &HirRvalue) {
        match rvalue {
            HirRvalue::Use(op) => self.print_operand(op),
            HirRvalue::BinaryOp { op, lhs, rhs } => {
                write!(&mut self.output, "{:?}(", op).unwrap();
                self.print_operand(lhs);
                self.write(", ");
                self.print_operand(rhs);
                self.write(")");
            }
            HirRvalue::UnaryOp { op, operand } => {
                write!(&mut self.output, "{:?}(", op).unwrap();
                self.print_operand(operand);
                self.write(")");
            }
            HirRvalue::Ref { place, owned } => {
                if *owned {
                    self.write("own ^");
                } else {
                    self.write("^");
                }
                self.print_place(place);
            }
            HirRvalue::Deref(op) => {
                self.write("*");
                self.print_operand(op);
            }
            HirRvalue::Aggregate { kind, fields } => {
                match kind {
                    AggregateKind::Struct(id) => {
                        write!(&mut self.output, "struct#{}{{", id.0).unwrap();
                    }
                    AggregateKind::Union(id, variant) => {
                        write!(&mut self.output, "union#{}::{}{{", id.0, variant).unwrap();
                    }
                    AggregateKind::Array(ty, len) => {
                        write!(&mut self.output, "[{}; {}]{{", ty, len).unwrap();
                    }
                    AggregateKind::ErrorUnion(variant, _) => {
                        let variant_name = if *variant == 0 { "Ok" } else { "Err" };
                        write!(&mut self.output, ".{}{{", variant_name).unwrap();
                    }
                }
                for (i, field) in fields.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.print_operand(field);
                }
                self.write("}");
            }
            HirRvalue::Discriminant(place) => {
                self.write("discriminant(");
                self.print_place(place);
                self.write(")");
            }
            HirRvalue::ExtractPayload { place, variant_index } => {
                write!(&mut self.output, "extract_payload<{}>(", variant_index).unwrap();
                self.print_place(place);
                self.write(")");
            }
            HirRvalue::ArrayToSlice { array, .. } => {
                self.write("array_to_slice(");
                self.print_operand(array);
                self.write(")");
            }
        }
    }

    fn print_operand(&mut self, operand: &HirOperand) {
        match operand {
            HirOperand::Const(konst) => {
                write!(&mut self.output, "{:?}", konst).unwrap();
            }
            HirOperand::Place(place) => {
                self.print_place(place);
            }
            HirOperand::FuncRef(func_id) => {
                write!(&mut self.output, "fn#{}", func_id.0).unwrap();
            }
        }
    }

    fn print_place(&mut self, place: &HirPlace) {
        match place {
            HirPlace::Local(local) => {
                write!(&mut self.output, "_{}", local.0).unwrap();
            }
            HirPlace::Field { base, field } => {
                self.print_place(base);
                write!(&mut self.output, ".{}", field).unwrap();
            }
            HirPlace::Index { base, index } => {
                self.print_place(base);
                self.write("[");
                self.print_operand(index);
                self.write("]");
            }
            HirPlace::Deref(place) => {
                self.write("(*");
                self.print_place(place);
                self.write(")");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diag::Span;

    #[test]
    fn test_print_local() {
        let mut printer = Printer::new(PrinterConfig::default());
        printer.print_place(&HirPlace::Local(LocalId(0)));
        assert_eq!(printer.finish(), "_0");
    }

    #[test]
    fn test_print_field_access() {
        let mut printer = Printer::new(PrinterConfig::default());
        let place = HirPlace::Field {
            base: Box::new(HirPlace::Local(LocalId(1))),
            field: 2,
        };
        printer.print_place(&place);
        assert_eq!(printer.finish(), "_1.2");
    }

    #[test]
    fn test_print_deref() {
        let mut printer = Printer::new(PrinterConfig::default());
        let place = HirPlace::Deref(Box::new(HirPlace::Local(LocalId(3))));
        printer.print_place(&place);
        assert_eq!(printer.finish(), "(*_3)");
    }

    #[test]
    fn test_print_const_operand() {
        let mut printer = Printer::new(PrinterConfig::default());
        printer.print_operand(&HirOperand::Const(Const::Int(42)));
        let output = printer.finish();
        assert!(output.contains("42"));
    }

    #[test]
    fn test_print_place_operand() {
        let mut printer = Printer::new(PrinterConfig::default());
        printer.print_operand(&HirOperand::Place(Box::new(HirPlace::Local(LocalId(5)))));
        assert_eq!(printer.finish(), "_5");
    }

    #[test]
    fn test_print_binary_op() {
        let mut printer = Printer::new(PrinterConfig::default());
        let rvalue = HirRvalue::BinaryOp {
            op: BinOp::Add,
            lhs: HirOperand::Place(Box::new(HirPlace::Local(LocalId(1)))),
            rhs: HirOperand::Const(Const::Int(10)),
        };
        printer.print_rvalue(&rvalue);
        let output = printer.finish();
        assert!(output.contains("Add"));
        assert!(output.contains("_1"));
        assert!(output.contains("10"));
    }

    #[test]
    fn test_print_assign_stmt() {
        let mut printer = Printer::new(PrinterConfig::default());
        let stmt = HirStmt::Assign {
            lhs: HirPlace::Local(LocalId(0)),
            rhs: HirRvalue::Use(HirOperand::Const(Const::Int(5))),
            span: Span::new(crate::diag::FileId(0), 0, 0),
        };
        printer.print_stmt(&stmt);
        let output = printer.finish();
        assert!(output.contains("_0"));
        assert!(output.contains("="));
        assert!(output.contains("5"));
    }

    #[test]
    fn test_print_goto_terminator() {
        let mut printer = Printer::new(PrinterConfig::default());
        let term = HirTerminator::Goto(BlockId(2));
        printer.print_terminator(&term);
        assert_eq!(printer.finish().trim(), "goto bb2");
    }

    #[test]
    fn test_print_return_with_value() {
        let mut printer = Printer::new(PrinterConfig::default());
        let term = HirTerminator::Return(Some(HirOperand::Place(Box::new(HirPlace::Local(LocalId(0))))));
        printer.print_terminator(&term);
        let output = printer.finish();
        assert!(output.contains("return"));
        assert!(output.contains("_0"));
    }

    #[test]
    fn test_print_return_void() {
        let mut printer = Printer::new(PrinterConfig::default());
        let term = HirTerminator::Return(None);
        printer.print_terminator(&term);
        assert_eq!(printer.finish().trim(), "return");
    }
}