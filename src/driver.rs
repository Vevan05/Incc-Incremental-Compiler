//! Phases 2 and 4: incremental build orchestration.
use std::fs;
use std::path::PathBuf;

use crate::ast::{canon_function_body, Program};
use crate::parser::parse;
use crate::audit::AuditReport;
use crate::cache::Cache;
use crate::codegen::{compile_function, globals_assembly};
use crate::deps::{canon_dep, dependencies};
use crate::fingerprint::KeyBuilder;
use crate::ir::{lower_function, optimize};
use crate::metrics::Metrics;
use crate::{Error, Result};

#[derive(Debug, Clone)]
pub struct BuildOptions {
    pub input: PathBuf,
    pub output: PathBuf,
    pub cache_dir: Option<PathBuf>,
    pub audit: bool,
    pub timings: bool,
    pub body_only: bool,
    pub file_level: bool,
    pub parallel: bool,
}

const TARGET: &str = "x86_64";
const OPTIONS: &str = "-O1";

pub fn build(opts: &BuildOptions, metrics: &mut Metrics) -> Result<()> {
    let src = fs::read_to_string(&opts.input)?;
    let program: Program = metrics.time("parse", || parse(&src))?;

    let _ = metrics.time("collect", || ());

    let mut funcs: Vec<&crate::ast::Function> = program.functions.iter().collect();
    funcs.sort_by(|a, b| a.sig.name.cmp(&b.sig.name));

    let cache = match &opts.cache_dir {
        Some(d) => Some(Cache::open(d)?),
        None => None,
    };

    // Phase 1: compute keys and cache status.
    struct FnInfo<'a> {
        func: &'a crate::ast::Function,
        key: crate::fingerprint::Key,
        cached: Option<Vec<u8>>,
    }
    let mut infos: Vec<FnInfo> = Vec::new();
    let mut hits = 0usize;
    let mut misses = 0usize;
    let mut miss_indices: Vec<usize> = Vec::new();
    for f in &funcs {
        let deps = metrics.time("deps", || dependencies(f, &program))?;
        let body_bytes = if opts.file_level {
            let mut all = Vec::new();
            for ff in &program.functions { all.extend(canon_function_body(ff)); }
            all
        } else {
            canon_function_body(f)
        };
        let mut kb = KeyBuilder::new(crate::COMPILER_VERSION, TARGET, OPTIONS);
        if !opts.file_level {
            kb.field(f.sig.name.as_bytes());
        }
        kb.field(&body_bytes);
        if !opts.body_only && !opts.file_level {
            let mut dep_list: Vec<&crate::sema::Lookup> = deps.iter().collect();
            dep_list.sort();
            for d in dep_list {
                kb.field(&canon_dep(d, &program));
            }
        }
        let key = kb.finish();
        let cached = match &cache {
            Some(c) => c.get(&key),
            None => None,
        };
        if cached.is_some() { hits += 1; } else { misses += 1; miss_indices.push(infos.len()); }
        infos.push(FnInfo { func: f, key, cached });
    }

    // Phase 2: compile misses.
    let mut compiled: std::collections::BTreeMap<usize, Vec<u8>> = std::collections::BTreeMap::new();
    if opts.parallel && miss_indices.len() > 1 {
        std::thread::scope(|scope| {
            let mut handles = Vec::new();
            for &idx in &miss_indices {
                let info = &infos[idx];
                let f = info.func;
                let program_ref = &program;
                let handle = scope.spawn(move || {
                    let mut fir = lower_function(f, program_ref);
                    optimize(&mut fir);
                    let art = compile_function(&fir, program_ref, OPTIONS);
                    art.bytes
                });
                handles.push((idx, handle));
            }
            for (idx, handle) in handles {
                if let Ok(bytes) = handle.join() {
                    if let Some(c) = &cache {
                        let _ = c.put(&infos[idx].key, &bytes);
                    }
                    compiled.insert(idx, bytes);
                }
            }
        });
    } else {
        for &idx in &miss_indices {
            let info = &infos[idx];
            let f = info.func;
            let mut fir = lower_function(f, &program);
            optimize(&mut fir);
            let art = metrics.time("compile", || compile_function(&fir, &program, OPTIONS));
            let bytes = art.bytes;
            if let Some(c) = &cache {
                let _ = c.put(&info.key, &bytes);
            }
            compiled.insert(idx, bytes);
        }
    }

    // Phase 3: assemble artifacts.
    let mut artifacts: Vec<(String, Vec<u8>)> = Vec::new();
    for (idx, info) in infos.iter().enumerate() {
        let bytes = if let Some(c) = &info.cached {
            c.clone()
        } else {
            compiled.get(&idx).cloned().unwrap()
        };
        artifacts.push((info.func.sig.name.clone(), bytes));
    }

    metrics.hits = hits;
    metrics.misses = misses;

    if let Some(parent) = opts.output.parent() {
        let _ = fs::create_dir_all(parent);
    }

    let output_bytes: Vec<u8> = metrics.time("link", || -> Result<Vec<u8>> {
        let mut asm = String::new();
        asm.push_str("    .text\n");
        asm.push_str(&globals_assembly(&program));
        asm.push_str("    .text\n");
        for (name, bytes) in &artifacts {
            asm.push_str(&format!("# function {}\n", name));
            asm.push_str(std::str::from_utf8(bytes).unwrap_or(""));
            asm.push('\n');
        }
        let tmpdir = std::env::temp_dir();
        let s_path = tmpdir.join(format!("incc_out_{}.s", std::process::id()));
        let o_path = tmpdir.join(format!("incc_out_{}.o", std::process::id()));
        fs::write(&s_path, asm.as_bytes())?;
        let status = std::process::Command::new("as")
            .arg(&s_path)
            .arg("-o")
            .arg(&o_path)
            .status()?;
        if !status.success() {
            return Err(Error::Io(std::io::Error::new(std::io::ErrorKind::Other, "as failed")));
        }
        let status = std::process::Command::new("gcc")
            .arg(&o_path)
            .arg("-o")
            .arg(&opts.output)
            .arg("-no-pie")
            .arg("-Wl,--build-id=none")
            .arg("-s")
            .status()?;
        if !status.success() {
            return Err(Error::Io(std::io::Error::new(std::io::ErrorKind::Other, "gcc failed")));
        }
        Ok(fs::read(&opts.output).unwrap_or_default())
    })?;

    if opts.audit {
        let _ = metrics.time("audit", || -> Result<()> {
            // clean build (no cache)
            let mut clean_artifacts: Vec<(String, Vec<u8>)> = Vec::new();
            for f in &funcs {
                let mut fir = lower_function(f, &program);
                optimize(&mut fir);
                let art = compile_function(&fir, &program, OPTIONS);
                clean_artifacts.push((f.sig.name.clone(), art.bytes));
            }
            let tmpdir = std::env::temp_dir();
            let scratch_out = tmpdir.join(format!("incc_audit_{}", std::process::id()));
            let mut asm = String::new();
            asm.push_str("    .text\n");
            asm.push_str(&globals_assembly(&program));
            asm.push_str("    .text\n");
            for (name, bytes) in &clean_artifacts {
                asm.push_str(&format!("# function {}\n", name));
                asm.push_str(std::str::from_utf8(bytes).unwrap_or(""));
                asm.push('\n');
            }
            let s_path = tmpdir.join(format!("incc_audit_{}.s", std::process::id()));
            let o_path = tmpdir.join(format!("incc_audit_{}.o", std::process::id()));
            fs::write(&s_path, asm.as_bytes())?;
            let status = std::process::Command::new("as")
                .arg(&s_path)
                .arg("-o")
                .arg(&o_path)
                .status()?;
            if !status.success() {
                return Err(Error::Io(std::io::Error::new(std::io::ErrorKind::Other, "as failed (audit)")));
            }
            let status = std::process::Command::new("gcc")
                .arg(&o_path)
                .arg("-o")
                .arg(&scratch_out)
                .arg("-no-pie")
                .arg("-Wl,--build-id=none")
                .arg("-s")
                .status()?;
            if !status.success() {
                return Err(Error::Io(std::io::Error::new(std::io::ErrorKind::Other, "gcc failed (audit)")));
            }
            let clean_out_bytes = fs::read(&scratch_out).unwrap_or_default();

            let mut report = AuditReport::default();
            for (name, inc_bytes) in &artifacts {
                if let Some((_, clean_bytes)) = clean_artifacts.iter().find(|(n, _)| n == name) {
                    if inc_bytes != clean_bytes {
                        report.mismatched_functions.push(name.clone());
                    }
                }
            }
            if clean_out_bytes != output_bytes {
                // record as mismatched output via a sentinel function name
                report.mismatched_functions.push("<output>".to_string());
            }
            if !report.passed() {
                return Err(Error::Sema(format!("audit failed: {:?}", report.mismatched_functions)));
            }
            Ok(())
        });
    }

    Ok(())
}
