use std::path::PathBuf;
use std::process::ExitCode;

use incc::cache::Cache;
use incc::driver::{build, BuildOptions};
use incc::metrics::Metrics;

const DEFAULT_CACHE: &str = ".incc-cache";

fn usage() -> ExitCode {
    eprintln!(
        "usage:\n  incc build <input> -o <output> [--no-cache] [--audit] [--timings] [--cache-dir DIR]\n  incc cache clear [--cache-dir DIR]"
    );
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("build") => cmd_build(&args[1..]),
        Some("cache") if args.get(1).map(String::as_str) == Some("clear") => {
            let dir = flag_value(&args, "--cache-dir").unwrap_or_else(|| DEFAULT_CACHE.into());
            match Cache::open(&dir).and_then(|c| c.clear()) {
                Ok(()) => {
                    println!("cleared {dir}");
                    ExitCode::SUCCESS
                }
                Err(e) => fail(e),
            }
        }
        _ => usage(),
    }
}

fn cmd_build(args: &[String]) -> ExitCode {
    let Some(input) = args.first().filter(|a| !a.starts_with('-')) else { return usage() };
    let output = flag_value(args, "-o").unwrap_or_else(|| "a.out".into());
    let no_cache = args.iter().any(|a| a == "--no-cache");
    let body_only = args.iter().any(|a| a == "--body-only-key");
    let file_level = args.iter().any(|a| a == "--file-level-cache");
    let parallel = args.iter().any(|a| a == "--parallel");
    let opts = BuildOptions {
        input: PathBuf::from(input),
        output: PathBuf::from(output),
        cache_dir: (!no_cache).then(|| PathBuf::from(flag_value(args, "--cache-dir").unwrap_or_else(|| DEFAULT_CACHE.into()))),
        audit: args.iter().any(|a| a == "--audit"),
        timings: args.iter().any(|a| a == "--timings"),
        body_only: body_only,
        file_level: file_level,
        parallel: parallel,
    };
    let mut metrics = Metrics::default();
    let result = build(&opts, &mut metrics);
    if opts.timings {
        eprint!("{}", metrics.report());
    }
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => fail(e),
    }
}

fn flag_value(args: &[String], flag: &str) -> Option<String> {
    args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned()
}

fn fail(e: incc::Error) -> ExitCode {
    eprintln!("error: {e}");
    ExitCode::FAILURE
}
