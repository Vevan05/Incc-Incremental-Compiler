//! Phase 1/2: backend (x86-64 assembly text).
use std::collections::BTreeMap;
use crate::ir::{FunctionIR, Instr, Value};
use crate::ast::{BinOp, Expr, Program, UnOp};

pub struct Artifact {
    pub name: String,
    pub bytes: Vec<u8>,
}

pub fn compile_function(fir: &FunctionIR, program: &Program, _options: &str) -> Artifact {
    // collect slots
    let mut slots: BTreeMap<String, usize> = BTreeMap::new();
    let mut next_slot = 0usize;
    let mut assign_slot = |slots: &mut BTreeMap<String, usize>, next: &mut usize, key: String| {
        if !slots.contains_key(&key) {
            slots.insert(key, *next);
            *next += 1;
        }
    };
    for p in &fir.params {
        assign_slot(&mut slots, &mut next_slot, format!("v:{}", p));
    }
    for l in &fir.locals {
        assign_slot(&mut slots, &mut next_slot, format!("v:{}", l));
    }
    // scan instrs
    for instr in &fir.instrs {
        collect_slots(instr, &mut slots, &mut next_slot, &mut assign_slot);
    }
    let nslots = next_slot;

    let mut out = String::new();
    out.push_str(&format!("    .globl {}\n", fir.name));
    out.push_str(&format!("{}:\n", fir.name));
    out.push_str("    pushq %rbp\n");
    out.push_str("    movq %rsp, %rbp\n");
    if nslots > 0 {
        out.push_str(&format!("    subq ${}, %rsp\n", nslots * 8));
    }
    // params from registers
    let arg_regs = ["%rdi", "%rsi", "%rdx", "%rcx", "%r8", "%r9"];
    for (i, p) in fir.params.iter().enumerate() {
        if i < 6 {
            let off = slot_offset(slots[&format!("v:{}", p)]);
            out.push_str(&format!("    movq {}, {}(%rbp)\n", arg_regs[i], off));
        }
    }

    let mut label_counter = 0usize;
    for instr in &fir.instrs {
        gen_instr(instr, &slots, &mut out, &mut label_counter, program);
    }
    // default epilogue
    out.push_str("    movq $0, %rax\n");
    out.push_str("    leaveq\n");
    out.push_str("    ret\n");

    Artifact { name: fir.name.clone(), bytes: out.into_bytes() }
}

fn collect_slots(instr: &Instr, slots: &mut BTreeMap<String, usize>, next: &mut usize, assign: &mut dyn FnMut(&mut BTreeMap<String,usize>, &mut usize, String)) {
    let mut add_val = |v: &Value| {
        if let Value::Temp(t) = v {
            assign(slots, next, format!("t:{}", t));
        } else if let Value::Var(n) = v {
            assign(slots, next, format!("v:{}", n));
        }
    };
    match instr {
        Instr::Assign { dst, src } => { add_val(dst); add_val(src); }
        Instr::BinOp { dst, l, r, .. } => { add_val(dst); add_val(l); add_val(r); }
        Instr::UnOp { dst, src, .. } => { add_val(dst); add_val(src); }
        Instr::Call { dst, args, .. } => { if let Some(d) = dst { add_val(d); } for a in args { add_val(a); } }
        Instr::LoadGlobal { dst, .. } => { add_val(dst); }
        Instr::StoreGlobal { src, .. } => { add_val(src); }
        Instr::CondJump { cond, .. } => { add_val(cond); }
        Instr::Return(Some(v)) => { add_val(v); }
        _ => {}
    }
}

fn slot_offset(idx: usize) -> i64 {
    -((idx as i64) + 1) * 8
}

fn slot_str(slots: &BTreeMap<String, usize>, v: &Value) -> String {
    match v {
        Value::Temp(t) => format!("{}(%rbp)", slot_offset(slots[&format!("t:{}", t)])),
        Value::Var(n) => format!("{}(%rbp)", slot_offset(slots[&format!("v:{}", n)])),
        _ => String::new(),
    }
}

fn gen_instr(instr: &Instr, slots: &BTreeMap<String, usize>, out: &mut String, _lc: &mut usize, _program: &Program) {
    match instr {
        Instr::Assign { dst, src } => {
            match src {
                Value::ConstInt(n) => {
                    out.push_str(&format!("    movq ${}, %rax\n", n));
                }
                Value::ConstBool(b) => {
                    out.push_str(&format!("    movq ${}, %rax\n", if *b { 1 } else { 0 }));
                }
                _ => {
                    out.push_str(&format!("    movq {}, %rax\n", slot_str(slots, src)));
                }
            }
            if let Value::Var(_) | Value::Temp(_) = dst {
                out.push_str(&format!("    movq %rax, {}\n", slot_str(slots, dst)));
            }
        }
        Instr::BinOp { op, dst, l, r } => {
            // load l
            match l {
                Value::ConstInt(n) => out.push_str(&format!("    movq ${}, %rax\n", n)),
                Value::ConstBool(b) => out.push_str(&format!("    movq ${}, %rax\n", if *b {1}else{0})),
                _ => out.push_str(&format!("    movq {}, %rax\n", slot_str(slots, l))),
            }
            // need r in rcx or immediate
            let opstr = match op {
                BinOp::Add => {
                    match r {
                        Value::ConstInt(n) => out.push_str(&format!("    addq ${}, %rax\n", n)),
                        Value::ConstBool(b) => out.push_str(&format!("    addq ${}, %rax\n", if *b{1}else{0})),
                        _ => { out.push_str(&format!("    movq {}, %rcx\n", slot_str(slots, r))); out.push_str("    addq %rcx, %rax\n"); }
                    }
                    ""
                }
                BinOp::Sub => {
                    match r {
                        Value::ConstInt(n) => out.push_str(&format!("    subq ${}, %rax\n", n)),
                        Value::ConstBool(b) => out.push_str(&format!("    subq ${}, %rax\n", if *b{1}else{0})),
                        _ => { out.push_str(&format!("    movq {}, %rcx\n", slot_str(slots, r))); out.push_str("    subq %rcx, %rax\n"); }
                    }
                    ""
                }
                BinOp::Mul => {
                    match r {
                        Value::ConstInt(n) => out.push_str(&format!("    imulq ${}, %rax\n", n)),
                        Value::ConstBool(b) => out.push_str(&format!("    imulq ${}, %rax\n", if *b{1}else{0})),
                        _ => { out.push_str(&format!("    movq {}, %rcx\n", slot_str(slots, r))); out.push_str("    imulq %rcx, %rax\n"); }
                    }
                    ""
                }
                BinOp::Div => {
                    // rax / r -> rax
                    // move r to rcx, then idiv
                    match r {
                        Value::ConstInt(n) => out.push_str(&format!("    movq ${}, %rcx\n", n)),
                        Value::ConstBool(b) => out.push_str(&format!("    movq ${}, %rcx\n", if *b{1}else{0})),
                        _ => out.push_str(&format!("    movq {}, %rcx\n", slot_str(slots, r))),
                    }
                    out.push_str("    cqto\n    idivq %rcx\n");
                    ""
                }
                BinOp::Mod => {
                    match r {
                        Value::ConstInt(n) => out.push_str(&format!("    movq ${}, %rcx\n", n)),
                        Value::ConstBool(b) => out.push_str(&format!("    movq ${}, %rcx\n", if *b{1}else{0})),
                        _ => out.push_str(&format!("    movq {}, %rcx\n", slot_str(slots, r))),
                    }
                    out.push_str("    cqto\n    idivq %rcx\n    movq %rdx, %rax\n");
                    ""
                }
                BinOp::Eq => {
                    // compare rax with r
                    match r {
                        Value::ConstInt(n) => out.push_str(&format!("    cmpq ${}, %rax\n", n)),
                        Value::ConstBool(b) => out.push_str(&format!("    cmpq ${}, %rax\n", if *b{1}else{0})),
                        _ => { out.push_str(&format!("    movq {}, %rcx\n", slot_str(slots, r))); out.push_str("    cmpq %rcx, %rax\n"); }
                    }
                    out.push_str("    sete %al\n    movzbq %al, %rax\n");
                    ""
                }
                BinOp::Ne => {
                    match r {
                        Value::ConstInt(n) => out.push_str(&format!("    cmpq ${}, %rax\n", n)),
                        Value::ConstBool(b) => out.push_str(&format!("    cmpq ${}, %rax\n", if *b{1}else{0})),
                        _ => { out.push_str(&format!("    movq {}, %rcx\n", slot_str(slots, r))); out.push_str("    cmpq %rcx, %rax\n"); }
                    }
                    out.push_str("    setne %al\n    movzbq %al, %rax\n");
                    ""
                }
                BinOp::Lt => {
                    match r {
                        Value::ConstInt(n) => out.push_str(&format!("    cmpq ${}, %rax\n", n)),
                        Value::ConstBool(b) => out.push_str(&format!("    cmpq ${}, %rax\n", if *b{1}else{0})),
                        _ => { out.push_str(&format!("    movq {}, %rcx\n", slot_str(slots, r))); out.push_str("    cmpq %rcx, %rax\n"); }
                    }
                    out.push_str("    setl %al\n    movzbq %al, %rax\n");
                    ""
                }
                BinOp::Le => {
                    match r {
                        Value::ConstInt(n) => out.push_str(&format!("    cmpq ${}, %rax\n", n)),
                        Value::ConstBool(b) => out.push_str(&format!("    cmpq ${}, %rax\n", if *b{1}else{0})),
                        _ => { out.push_str(&format!("    movq {}, %rcx\n", slot_str(slots, r))); out.push_str("    cmpq %rcx, %rax\n"); }
                    }
                    out.push_str("    setle %al\n    movzbq %al, %rax\n");
                    ""
                }
                BinOp::Gt => {
                    match r {
                        Value::ConstInt(n) => out.push_str(&format!("    cmpq ${}, %rax\n", n)),
                        Value::ConstBool(b) => out.push_str(&format!("    cmpq ${}, %rax\n", if *b{1}else{0})),
                        _ => { out.push_str(&format!("    movq {}, %rcx\n", slot_str(slots, r))); out.push_str("    cmpq %rcx, %rax\n"); }
                    }
                    out.push_str("    setg %al\n    movzbq %al, %rax\n");
                    ""
                }
                BinOp::Ge => {
                    match r {
                        Value::ConstInt(n) => out.push_str(&format!("    cmpq ${}, %rax\n", n)),
                        Value::ConstBool(b) => out.push_str(&format!("    cmpq ${}, %rax\n", if *b{1}else{0})),
                        _ => { out.push_str(&format!("    movq {}, %rcx\n", slot_str(slots, r))); out.push_str("    cmpq %rcx, %rax\n"); }
                    }
                    out.push_str("    setge %al\n    movzbq %al, %rax\n");
                    ""
                }
                BinOp::And => {
                    // normalize rax to 0/1
                    out.push_str("    testq %rax, %rax\n    setne %al\n    movzbq %al, %rax\n");
                    match r {
                        Value::ConstInt(n) => out.push_str(&format!("    movq ${}, %rcx\n", if *n != 0 {1}else{0})),
                        Value::ConstBool(b) => out.push_str(&format!("    movq ${}, %rcx\n", if *b {1}else{0})),
                        _ => { out.push_str(&format!("    movq {}, %rcx\n", slot_str(slots, r))); out.push_str("    testq %rcx, %rcx\n    setne %cl\n    movzbq %cl, %rcx\n"); }
                    }
                    out.push_str("    andq %rcx, %rax\n");
                    ""
                }
                BinOp::Or => {
                    out.push_str("    testq %rax, %rax\n    setne %al\n    movzbq %al, %rax\n");
                    match r {
                        Value::ConstInt(n) => out.push_str(&format!("    movq ${}, %rcx\n", if *n != 0 {1}else{0})),
                        Value::ConstBool(b) => out.push_str(&format!("    movq ${}, %rcx\n", if *b {1}else{0})),
                        _ => { out.push_str(&format!("    movq {}, %rcx\n", slot_str(slots, r))); out.push_str("    testq %rcx, %rcx\n    setne %cl\n    movzbq %cl, %rcx\n"); }
                    }
                    out.push_str("    orq %rcx, %rax\n");
                    ""
                }
            };
            let _ = opstr;
            out.push_str(&format!("    movq %rax, {}\n", slot_str(slots, dst)));
        }
        Instr::UnOp { op, dst, src } => {
            match src {
                Value::ConstInt(n) => out.push_str(&format!("    movq ${}, %rax\n", n)),
                Value::ConstBool(b) => out.push_str(&format!("    movq ${}, %rax\n", if *b {1}else{0})),
                _ => out.push_str(&format!("    movq {}, %rax\n", slot_str(slots, src))),
            }
            match op {
                UnOp::Neg => out.push_str("    negq %rax\n"),
                UnOp::Not => { out.push_str("    testq %rax, %rax\n    sete %al\n    movzbq %al, %rax\n"); }
            }
            out.push_str(&format!("    movq %rax, {}\n", slot_str(slots, dst)));
        }
        Instr::Call { dst, name, args } => {
            let arg_regs = ["%rdi", "%rsi", "%rdx", "%rcx", "%r8", "%r9"];
            for (i, a) in args.iter().enumerate() {
                if i < 6 {
                    match a {
                        Value::ConstInt(n) => out.push_str(&format!("    movq ${}, {}\n", n, arg_regs[i])),
                        Value::ConstBool(b) => out.push_str(&format!("    movq ${}, {}\n", if *b{1}else{0}, arg_regs[i])),
                        _ => out.push_str(&format!("    movq {}, {}\n", slot_str(slots, a), arg_regs[i])),
                    }
                }
            }
            out.push_str(&format!("    call {}\n", name));
            if let Some(d) = dst {
                out.push_str(&format!("    movq %rax, {}\n", slot_str(slots, d)));
            }
        }
        Instr::LoadGlobal { dst, name } => {
            out.push_str(&format!("    movq {}(%rip), %rax\n", name));
            out.push_str(&format!("    movq %rax, {}\n", slot_str(slots, dst)));
        }
        Instr::StoreGlobal { name, src } => {
            match src {
                Value::ConstInt(n) => out.push_str(&format!("    movq ${}, %rax\n", n)),
                Value::ConstBool(b) => out.push_str(&format!("    movq ${}, %rax\n", if *b{1}else{0})),
                _ => out.push_str(&format!("    movq {}, %rax\n", slot_str(slots, src))),
            }
            out.push_str(&format!("    movq %rax, {}(%rip)\n", name));
        }
        Instr::Label(l) => out.push_str(&format!("{}:\n", l)),
        Instr::Jump(l) => out.push_str(&format!("    jmp {}\n", l)),
        Instr::CondJump { cond, t, f } => {
            match cond {
                Value::ConstInt(n) => out.push_str(&format!("    movq ${}, %rax\n", n)),
                Value::ConstBool(b) => out.push_str(&format!("    movq ${}, %rax\n", if *b{1}else{0})),
                _ => out.push_str(&format!("    movq {}, %rax\n", slot_str(slots, cond))),
            }
            out.push_str("    testq %rax, %rax\n");
            out.push_str(&format!("    jne {}\n", t));
            out.push_str(&format!("    jmp {}\n", f));
        }
        Instr::Return(Some(v)) => {
            match v {
                Value::ConstInt(n) => out.push_str(&format!("    movq ${}, %rax\n", n)),
                Value::ConstBool(b) => out.push_str(&format!("    movq ${}, %rax\n", if *b{1}else{0})),
                _ => out.push_str(&format!("    movq {}, %rax\n", slot_str(slots, v))),
            }
            out.push_str("    leaveq\n    ret\n");
        }
        Instr::Return(None) => {
            out.push_str("    movq $0, %rax\n    leaveq\n    ret\n");
        }
    }
}

// globals assembly helper
pub fn globals_assembly(program: &Program) -> String {
    let mut out = String::new();
    out.push_str("    .bss\n");
    for g in &program.globals {
        out.push_str(&format!("    .comm {}, 8, 8\n", g.name));
    }
    // const values not stored
    let _ = program;
    out
}

// evaluate global init not handled in assembly; default zero.
pub fn _unused(e: &Expr) { let _ = e; }
