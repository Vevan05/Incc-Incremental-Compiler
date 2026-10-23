//! Phase 3: dependency extraction. Thin layer over `sema::InterfaceView`'s recorded lookups,
//! plus canonical serialization of a dependency and a human-readable "why was this rebuilt" report.
use crate::ast::{canon_program_consts, canon_program_global, canon_type, Program};
use crate::sema::Lookup;

pub fn dependencies(f: &crate::ast::Function, program: &Program) -> crate::Result<std::collections::BTreeSet<Lookup>> {
    crate::sema::dependencies(f, program)
}

pub fn canon_dep(dep: &Lookup, program: &Program) -> Vec<u8> {
    use crate::ast::{push_str, push_u8, push_u64};
    let mut out = Vec::new();
    match dep {
        Lookup::Function(name) => {
            push_u8(&mut out, 1);
            push_str(&mut out, name);
            if let Some(f) = program.functions.iter().find(|f| f.sig.name == *name) {
                push_u64(&mut out, f.sig.params.len() as u64);
                for p in &f.sig.params { canon_type(&p.ty, &mut out); push_str(&mut out, &p.name); }
                canon_type(&f.sig.ret, &mut out);
            }
        }
        Lookup::Const(name) => {
            push_u8(&mut out, 2);
            push_str(&mut out, name);
            if let Some(c) = program.consts.iter().find(|c| c.name == *name) {
                canon_program_consts(c).into_iter().for_each(|b| out.push(b));
            }
        }
        Lookup::Global(name) => {
            push_u8(&mut out, 3);
            push_str(&mut out, name);
            if let Some(g) = program.globals.iter().find(|g| g.name == *name) {
                canon_program_global(g).into_iter().for_each(|b| out.push(b));
            }
        }
        Lookup::Missing(name) => {
            push_u8(&mut out, 4);
            push_str(&mut out, name);
        }
    }
    out
}

pub fn why_rebuilt(name: &str, deps: &std::collections::BTreeSet<Lookup>) -> String {
    let mut s = format!("function {name} rebuilt because of:\n");
    for d in deps {
        s.push_str(&format!("  - {:?}\n", d));
    }
    s
}

