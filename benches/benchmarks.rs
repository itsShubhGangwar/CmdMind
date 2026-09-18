//! Phase 8 High-Precision Performance Benchmark Suite for CmdMind
//!
//! Measures actual runtime performance for:
//! 1. Tier-1 Intent Engine (`match_intent`)
//! 2. Security Validator (`validate` on safe & rejected commands)
//! 3. Levenshtein Distance (`levenshtein` across short, medium, and long strings)
//! 4. Auto-Healing Path Resolver (`resolve_paths` on isolated test fixtures)
//! 5. SQLite Persistence (`open_in_memory`, `save_command`, `get_recent_history`)
//! 6. End-to-End Deterministic Pipeline (Request ──► ValidatedPlan ──► Path Resolver ──► DB)

use std::fs::{self, File};
use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::time::Instant;

use cmdmind::db::Database;
use cmdmind::intent::{match_intent, CommandPlan, CommandSource};
use cmdmind::resolver::{levenshtein, resolve_paths};
use cmdmind::security::validate;
use cmdmind::zsh::prepare_for_zsh;

#[derive(Debug, Clone)]
struct BenchmarkStats {
    name: String,
    #[allow(dead_code)]
    iterations: usize,
    min_ns: f64,
    max_ns: f64,
    mean_ns: f64,
    median_ns: f64,
    std_dev_ns: f64,
}

impl BenchmarkStats {
    fn format_time(ns: f64) -> String {
        if ns < 1_000.0 {
            format!("{:.1} ns", ns)
        } else if ns < 1_000_000.0 {
            format!("{:.2} µs", ns / 1_000.0)
        } else {
            format!("{:.3} ms", ns / 1_000_000.0)
        }
    }

    fn print_row(&self) {
        println!(
            "{:<48} | {:>10} | {:>10} | {:>10} | {:>10} | {:>10}",
            self.name,
            Self::format_time(self.median_ns),
            Self::format_time(self.mean_ns),
            Self::format_time(self.min_ns),
            Self::format_time(self.max_ns),
            Self::format_time(self.std_dev_ns)
        );
    }
}

fn run_benchmark<F>(
    name: &str,
    warmup_iters: usize,
    sample_count: usize,
    iters_per_sample: usize,
    mut f: F,
) -> BenchmarkStats
where
    F: FnMut(),
{
    // Warmup phase: stabilize branch predictors and CPU caches
    for _ in 0..warmup_iters {
        f();
    }

    let mut samples_ns = Vec::with_capacity(sample_count);

    for _ in 0..sample_count {
        let start = Instant::now();
        for _ in 0..iters_per_sample {
            f();
        }
        let elapsed = start.elapsed();
        let per_iter_ns = elapsed.as_nanos() as f64 / iters_per_sample as f64;
        samples_ns.push(per_iter_ns);
    }

    samples_ns.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let min_ns = samples_ns[0];
    let max_ns = samples_ns[sample_count - 1];
    let median_ns = if sample_count % 2 == 0 {
        (samples_ns[sample_count / 2 - 1] + samples_ns[sample_count / 2]) / 2.0
    } else {
        samples_ns[sample_count / 2]
    };
    let sum: f64 = samples_ns.iter().sum();
    let mean_ns = sum / sample_count as f64;

    let variance: f64 = samples_ns
        .iter()
        .map(|v| (v - mean_ns).powi(2))
        .sum::<f64>()
        / sample_count as f64;
    let std_dev_ns = variance.sqrt();

    BenchmarkStats {
        name: name.to_string(),
        iterations: sample_count * iters_per_sample,
        min_ns,
        max_ns,
        mean_ns,
        median_ns,
        std_dev_ns,
    }
}

// ============================================================================
// Benchmarking Fixtures for Path Resolver
// ============================================================================

struct BenchDir {
    path: PathBuf,
}

impl BenchDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "cmdmind_bench_{}_{}_{}",
            name,
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }

    fn create_dir(&self, rel: &str) {
        fs::create_dir_all(self.path.join(rel)).unwrap();
    }

    fn create_file(&self, rel: &str) {
        let full = self.path.join(rel);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        File::create(full).unwrap();
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for BenchDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

// ============================================================================
// Main Benchmark Runner
// ============================================================================

fn main() {
    println!("\nCmdMind Phase 8 Benchmark Suite");
    println!("===============================================================================================================");
    println!(
        "{:<48} | {:>10} | {:>10} | {:>10} | {:>10} | {:>10}",
        "Benchmark Target", "Median", "Mean", "Min", "Max", "StdDev"
    );
    println!("-------------------------------------------------+------------+------------+------------+------------+------------");

    let mut results = Vec::new();

    // 1. Tier-1 Intent Engine Benchmarks
    results.push(run_benchmark(
        "Tier-1: match_intent(\"find all pdf files\")",
        1000,
        500,
        50,
        || {
            let res = match_intent(black_box("find all pdf files"));
            black_box(res);
        },
    ));

    results.push(run_benchmark(
        "Tier-1: match_intent(\"git status\")",
        1000,
        500,
        50,
        || {
            let res = match_intent(black_box("git status"));
            black_box(res);
        },
    ));

    results.push(run_benchmark(
        "Tier-1: match_intent(\"show files\")",
        1000,
        500,
        50,
        || {
            let res = match_intent(black_box("show files"));
            black_box(res);
        },
    ));

    results.push(run_benchmark(
        "Tier-1: match_intent(unknown request)",
        1000,
        500,
        50,
        || {
            let res = match_intent(black_box("inspect network sockets and routing table"));
            black_box(res);
        },
    ));

    // 2. Security Validator Benchmarks (Safe Commands)
    results.push(run_benchmark(
        "Validator (Safe): validate(\"ls\")",
        1000,
        500,
        50,
        || {
            let plan = CommandPlan::new(black_box("ls"), "List files", CommandSource::Tier1);
            let res = validate(plan);
            black_box(res).unwrap();
        },
    ));

    results.push(run_benchmark(
        "Validator (Safe): validate(\"find . -type f -name '*.pdf'\")",
        1000,
        500,
        50,
        || {
            let plan = CommandPlan::new(
                black_box("find . -type f -name '*.pdf'"),
                "Find PDFs",
                CommandSource::Tier1,
            );
            let res = validate(plan);
            black_box(res).unwrap();
        },
    ));

    results.push(run_benchmark(
        "Validator (Safe): validate(\"find . -mtime -2\")",
        1000,
        500,
        50,
        || {
            let plan = CommandPlan::new(
                black_box("find . -type f -name '*.py' -mtime -2"),
                "Find Python",
                CommandSource::Tier1,
            );
            let res = validate(plan);
            black_box(res).unwrap();
        },
    ));

    // 2b. Security Validator Benchmarks (Rejected Commands)
    results.push(run_benchmark(
        "Validator (Reject): validate(\"rm -rf /\")",
        1000,
        500,
        50,
        || {
            let plan = CommandPlan::new(black_box("rm -rf /"), "Malicious", CommandSource::Tier1);
            let res = validate(plan);
            assert!(black_box(res).is_err());
        },
    ));

    results.push(run_benchmark(
        "Validator (Reject): validate(\"ls && rm file\")",
        1000,
        500,
        50,
        || {
            let plan =
                CommandPlan::new(black_box("ls && rm file"), "Chaining", CommandSource::Tier1);
            let res = validate(plan);
            assert!(black_box(res).is_err());
        },
    ));

    results.push(run_benchmark(
        "Validator (Reject): validate(\"find /etc -type f\")",
        1000,
        500,
        50,
        || {
            let plan = CommandPlan::new(
                black_box("find /etc -type f"),
                "Root path",
                CommandSource::Tier1,
            );
            let res = validate(plan);
            assert!(black_box(res).is_err());
        },
    ));

    // 3. Levenshtein Distance Benchmarks
    results.push(run_benchmark(
        "Levenshtein: short (\"src\" vs \"srcc\")",
        2000,
        1000,
        50,
        || {
            let d = levenshtein(black_box("src"), black_box("srcc"));
            black_box(d);
        },
    ));

    results.push(run_benchmark(
        "Levenshtein: medium (\"documents\" vs \"documnts\")",
        2000,
        1000,
        50,
        || {
            let d = levenshtein(black_box("documents"), black_box("documnts"));
            black_box(d);
        },
    ));

    results.push(run_benchmark(
        "Levenshtein: substitutions (\"kitten\" vs \"sitting\")",
        2000,
        1000,
        50,
        || {
            let d = levenshtein(black_box("kitten"), black_box("sitting"));
            black_box(d);
        },
    ));

    results.push(run_benchmark(
        "Levenshtein: long (50 chars vs 50 chars, distance = 4)",
        1000,
        500,
        20,
        || {
            let a = "cmdmind_system_architecture_security_test_string_a";
            let b = "cmdmind_system_architecture_security_test_string_b";
            let d = levenshtein(black_box(a), black_box(b));
            black_box(d);
        },
    ));

    // 4. Path Resolver Benchmarks (Isolated Temporary Filesystem)
    let bench_dir = BenchDir::new("resolver");
    bench_dir.create_dir("src");
    bench_dir.create_dir("docs");
    bench_dir.create_dir("tests");
    bench_dir.create_dir("src1");
    bench_dir.create_dir("src2");
    bench_dir.create_file("README.md");

    let plan_existing = validate(CommandPlan::new(
        "find src -type f",
        "Test",
        CommandSource::Tier1,
    ))
    .unwrap();
    results.push(run_benchmark(
        "Path Resolver: existing path (\"src\")",
        500,
        200,
        10,
        || {
            let res = resolve_paths(black_box(&plan_existing), Some(bench_dir.path()));
            black_box(res);
        },
    ));

    let plan_typo = validate(CommandPlan::new(
        "find srcc -type f",
        "Test",
        CommandSource::Tier1,
    ))
    .unwrap();
    results.push(run_benchmark(
        "Path Resolver: typo with match (\"srcc\" -> \"src\")",
        200,
        100,
        5,
        || {
            let res = resolve_paths(black_box(&plan_typo), Some(bench_dir.path()));
            black_box(res);
        },
    ));

    let plan_nomatch = validate(CommandPlan::new(
        "find totally_unrelated_xyz_123 -type f",
        "Test",
        CommandSource::Tier1,
    ))
    .unwrap();
    results.push(run_benchmark(
        "Path Resolver: missing path no match",
        200,
        100,
        5,
        || {
            let res = resolve_paths(black_box(&plan_nomatch), Some(bench_dir.path()));
            black_box(res);
        },
    ));

    // 5. SQLite Persistence Benchmarks
    results.push(run_benchmark(
        "SQLite: open_in_memory() initialization",
        100,
        50,
        5,
        || {
            let db = Database::open_in_memory().unwrap();
            black_box(db);
        },
    ));

    let db_bench = Database::open_in_memory().unwrap();
    let plan_save = validate(CommandPlan::new(
        "find . -name '*.pdf'",
        "PDF",
        CommandSource::Tier1,
    ))
    .unwrap();
    results.push(run_benchmark(
        "SQLite: insert history entry (`save_command`)",
        100,
        100,
        5,
        || {
            let id = db_bench
                .save_command(black_box("find all pdf files"), black_box(&plan_save))
                .unwrap();
            black_box(id);
        },
    ));

    results.push(run_benchmark(
        "SQLite: retrieve history (`get_recent_history(20)`)",
        100,
        100,
        5,
        || {
            let entries = db_bench.get_recent_history(black_box(20)).unwrap();
            black_box(entries);
        },
    ));

    // 6. End-to-End Deterministic Pipeline Benchmark
    let db_e2e = Database::open_in_memory().unwrap();
    results.push(run_benchmark(
        "End-to-End Pipeline: Tier-1 -> Valid -> Resolve -> DB",
        200,
        100,
        5,
        || {
            let req = black_box("find all pdf files");
            let plan = match_intent(req).unwrap();
            let validated = validate(plan).unwrap();
            let _resolved = resolve_paths(&validated, Some(bench_dir.path()));
            let zsh_cmd = prepare_for_zsh(validated.clone());
            black_box(&zsh_cmd);
            db_e2e.save_command(req, &validated).unwrap();
        },
    ));

    // Print all rows
    for r in &results {
        r.print_row();
    }
    println!("===============================================================================================================\n");
}
