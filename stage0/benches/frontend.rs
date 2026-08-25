//! 前端 pass 性能基准。ADR 010：测量确定性计数器，Criterion 记录执行时间。
//!
//! 每个基准完成后打印 FrontendCounters，便于跨 commit 对比工作量。

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use kore_stage0::diag::{DiagSink, FileId};
use kore_stage0::frontend::counters::FrontendCounters;
use kore_stage0::frontend::lexer::tokenize;
use kore_stage0::frontend::parser::parse;

// ---------- 测试源码 ----------

/// ~100 行 Kore0 源码（中等规模）
const SRC_MEDIUM: &str = r#"
add :: (a i32, b i32) i32 => a + b

factorial :: (n i32) i32 => ? {
    n == 0 => 1
    _ => n * factorial(n - 1)
}

fib :: (n i32) i32 => ? {
    n == 0 => 0
    n == 1 => 1
    _ => fib(n - 1) + fib(n - 2)
}

Vec3 :: {x, y, z f32}

dot :: (a ^Vec3, b ^Vec3) f32 =>
    a.x * b.x + a.y * b.y + a.z * b.z

norm_sq :: (v ^Vec3) f32 => dot(v, v)

clamp :: (x f32, lo f32, hi f32) f32 => ? {
    x < lo => lo
    x > hi => hi
    _ => x
}

Shape :: .Circle(f32) | .Rect(f32, f32) | .Triangle(f32, f32, f32)

area :: (s Shape) f32 => ? s {
    .Circle(r) => r * r * 3
    .Rect(w, h) => w * h
    .Triangle(a, b, c) => {
        s : f32 = (a + b + c) / 2
        s * (s - a) * (s - b) * (s - c)
    }
}

sum_array :: (arr ^[8]i32) i32 => {
    total : i32 = 0
    @ arr {
        .item(x) => total = total + x
    }
    total
}

max_of_three :: (a i32, b i32, c i32) i32 => ? {
    a >= b && a >= c => a
    b >= c           => b
    _                => c
}

is_prime :: (n i32) i32 => ? {
    n < 2 => 0
    _ => {
        i : i32 = 2
        result : i32 = 1
        @ {
            i * i > n => stop result
            n % i == 0 => { result = 0; stop result }
            _ => i = i + 1
        }
    }
}

gcd :: (a i32, b i32) i32 => ? {
    b == 0 => a
    _ => gcd(b, a % b)
}

lcm :: (a i32, b i32) i32 => a / gcd(a, b) * b
"#;

/// ~400 行 Kore0 源码（较大规模，重复 medium 4 次）
fn src_large() -> String {
    SRC_MEDIUM.repeat(4)
}

// ---------- 辅助函数 ----------

fn lex_src(src: &str) -> Vec<kore_stage0::frontend::lexer::Token> {
    let mut sink = DiagSink::new();
    tokenize(FileId(0), src, &mut sink)
}

fn parse_src(src: &str) -> kore_stage0::frontend::ast::Module {
    let mut sink = DiagSink::new();
    let tokens = tokenize(FileId(0), src, &mut sink);
    let mut sink2 = DiagSink::new();
    parse(FileId(0), tokens, &mut sink2)
}

// ---------- 基准组 ----------

fn bench_lex(c: &mut Criterion) {
    let large = src_large();

    let mut group = c.benchmark_group("lex");

    group.bench_function("medium", |b| {
        b.iter(|| {
            let mut sink = DiagSink::new();
            let tokens = tokenize(FileId(0), black_box(SRC_MEDIUM), &mut sink);
            black_box(tokens.len())
        })
    });

    group.bench_function("large", |b| {
        b.iter(|| {
            let mut sink = DiagSink::new();
            let tokens = tokenize(FileId(0), black_box(large.as_str()), &mut sink);
            black_box(tokens.len())
        })
    });

    // 打印确定性计数器
    let tokens_m = lex_src(SRC_MEDIUM);
    let tokens_l = lex_src(&large);
    eprintln!(
        "[counters] lex/medium  tokens={} | lex/large tokens={}",
        tokens_m.len(),
        tokens_l.len()
    );

    group.finish();
}

fn bench_parse(c: &mut Criterion) {
    let large = src_large();

    let mut group = c.benchmark_group("parse");

    group.bench_function("medium", |b| {
        b.iter(|| {
            let mut sink = DiagSink::new();
            let tokens = tokenize(FileId(0), black_box(SRC_MEDIUM), &mut sink);
            let mut sink2 = DiagSink::new();
            let module = parse(FileId(0), tokens, &mut sink2);
            black_box(module.items.len())
        })
    });

    group.bench_function("large", |b| {
        b.iter(|| {
            let mut sink = DiagSink::new();
            let tokens = tokenize(FileId(0), black_box(large.as_str()), &mut sink);
            let mut sink2 = DiagSink::new();
            let module = parse(FileId(0), tokens, &mut sink2);
            black_box(module.items.len())
        })
    });

    // 打印确定性计数器
    {
        let mut sink = DiagSink::new();
        let tokens_m = tokenize(FileId(0), SRC_MEDIUM, &mut sink);
        let module_m = parse_src(SRC_MEDIUM);
        let mut sink2 = DiagSink::new();
        let tokens_l = tokenize(FileId(0), &large, &mut sink2);
        let module_l = parse_src(&large);

        let cnt_m = FrontendCounters::from_outputs(&tokens_m, &module_m, 0, 0);
        let cnt_l = FrontendCounters::from_outputs(&tokens_l, &module_l, 0, 0);
        eprintln!(
            "[counters] parse/medium  tokens={} items={} exprs={} | parse/large tokens={} items={} exprs={}",
            cnt_m.tokens_produced, cnt_m.items_parsed, cnt_m.expr_nodes,
            cnt_l.tokens_produced, cnt_l.items_parsed, cnt_l.expr_nodes,
        );
    }

    group.finish();
}

criterion_group!(benches, bench_lex, bench_parse);
criterion_main!(benches);
