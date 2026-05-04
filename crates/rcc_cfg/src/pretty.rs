//! Stable textual MIR/CFG dump.

use std::fmt::Write as _;

use rcc_hir::{FloatKind, IntRank, Qual, Ty, TyCtxt, TyId};

use crate::{
    BasicBlockId, Body, Const, ConstKind, Local, Operand, Place, Projection, Rvalue, Statement,
    StatementKind, TerminatorKind, UnOp,
};

/// Dump one CFG body in a stable, snapshot-friendly text format.
#[must_use]
pub fn dump_body(body: &Body, tcx: &TyCtxt) -> String {
    let mut out = String::new();
    let def = body.def.map_or_else(|| "anon".to_string(), |d| format!("def#{}", d.0));
    let ret = body.ret_ty.map_or_else(|| "<unknown>".to_string(), |ty| fmt_ty(tcx, ty));
    let _ = writeln!(out, "fn {def}() -> {ret} {{");
    out.push_str("  locals:\n");
    for (local, decl) in body.locals.iter_enumerated() {
        let kind = if decl.is_param { " param" } else { "" };
        let name = decl.name.map_or_else(String::new, |s| format!(" name#{}", s.0));
        let vla = decl.vla_len.map_or_else(String::new, |l| format!(" vla_len={}", fmt_local(l)));
        let _ = writeln!(
            out,
            "    {}: {}{}{}{};",
            fmt_local(local),
            fmt_ty(tcx, decl.ty),
            kind,
            name,
            vla
        );
    }
    out.push('\n');
    for (bb, block) in body.blocks.iter_enumerated() {
        let _ = writeln!(out, "  {}:", fmt_bb(bb));
        for stmt in &block.statements {
            let _ = writeln!(out, "    {}", fmt_stmt(tcx, stmt));
        }
        let _ = writeln!(out, "    {}", fmt_term(&block.terminator.kind));
        out.push('\n');
    }
    out.push_str("}\n");
    out
}

fn fmt_stmt(tcx: &TyCtxt, stmt: &Statement) -> String {
    match &stmt.kind {
        StatementKind::Assign { place, rvalue } => {
            format!("{} = {};", fmt_place(place), fmt_rvalue(tcx, rvalue))
        }
        StatementKind::StorageLive(local) => format!("StorageLive({});", fmt_local(*local)),
        StatementKind::StorageDead(local) => format!("StorageDead({});", fmt_local(*local)),
        StatementKind::Nop => "nop;".to_string(),
    }
}

fn fmt_term(term: &TerminatorKind) -> String {
    match term {
        TerminatorKind::Goto(target) => format!("goto -> {};", fmt_bb(*target)),
        TerminatorKind::IndirectGoto { target, targets } => {
            let targets = targets.iter().map(|bb| fmt_bb(*bb)).collect::<Vec<_>>().join(", ");
            format!("indirect_goto {} -> [{targets}];", fmt_operand(target))
        }
        TerminatorKind::SwitchInt { discr, targets } => {
            let rendered = targets
                .iter()
                .map(|(value, target)| match value {
                    Some(v) => format!("{v}: {}", fmt_bb(*target)),
                    None => format!("otherwise: {}", fmt_bb(*target)),
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("switchInt({}) -> [{}];", fmt_operand(discr), rendered)
        }
        TerminatorKind::Return => "return;".to_string(),
        TerminatorKind::Call { callee, args, destination, target } => {
            let args = args.iter().map(fmt_operand).collect::<Vec<_>>().join(", ");
            let dest = destination.as_ref().map_or("_".to_string(), fmt_place);
            let target = target.map_or_else(|| "unwind".to_string(), fmt_bb);
            format!("call {}({args}) -> {dest}, {target};", fmt_operand(callee))
        }
        TerminatorKind::Unreachable => "unreachable;".to_string(),
        TerminatorKind::BuiltinVaStart { ap, last_param, target } => {
            format!(
                "va_start({}, {}) -> {};",
                fmt_operand(ap),
                fmt_operand(last_param),
                fmt_bb(*target)
            )
        }
        TerminatorKind::BuiltinVaEnd { ap, target } => {
            format!("va_end({}) -> {};", fmt_operand(ap), fmt_bb(*target))
        }
        TerminatorKind::BuiltinVaCopy { dst, src, target } => {
            format!("va_copy({}, {}) -> {};", fmt_operand(dst), fmt_operand(src), fmt_bb(*target))
        }
    }
}

fn fmt_rvalue(tcx: &TyCtxt, rvalue: &Rvalue) -> String {
    match rvalue {
        Rvalue::Use(op) => fmt_operand(op),
        Rvalue::BinaryOp(op, lhs, rhs) => {
            format!("{op:?}({}, {})", fmt_operand(lhs), fmt_operand(rhs))
        }
        Rvalue::UnaryOp(op, operand) => format!("{}({})", fmt_unop(*op), fmt_operand(operand)),
        Rvalue::Cast { op, to, kind } => {
            format!("Cast::{kind:?}({}, {})", fmt_operand(op), fmt_ty(tcx, *to))
        }
        Rvalue::ComplexFromReal { real, to } => {
            format!("ComplexFromReal({}, {})", fmt_operand(real), fmt_ty(tcx, *to))
        }
        Rvalue::RealFromComplex { complex, to } => {
            format!("RealFromComplex({}, {})", fmt_operand(complex), fmt_ty(tcx, *to))
        }
        Rvalue::AddressOf(place) => format!("&{}", fmt_place(place)),
        Rvalue::LoadGlobal { def, .. } => format!("load global#{}", def.0),
        Rvalue::Len(place) => format!("Len({})", fmt_place(place)),
        Rvalue::BuiltinVaArg { ap, ty } => {
            format!("va_arg({}, {})", fmt_operand(ap), fmt_ty(tcx, *ty))
        }
    }
}

fn fmt_operand(operand: &Operand) -> String {
    match operand {
        Operand::Copy(place) => format!("copy {}", fmt_place(place)),
        Operand::Move(place) => format!("move {}", fmt_place(place)),
        Operand::Const(c) => fmt_const(c),
    }
}

fn fmt_const(c: &Const) -> String {
    match &c.kind {
        ConstKind::Int(v) => v.to_string(),
        ConstKind::Float(v) => v.to_string(),
        ConstKind::Global(def) => format!("global#{}", def.0),
        ConstKind::BlockAddress(bb) => format!("blockaddress({})", fmt_bb(*bb)),
        ConstKind::ZeroInit => "ZeroInit".to_string(),
    }
}

fn fmt_place(place: &Place) -> String {
    let mut out = fmt_local(place.base);
    for proj in &place.projection {
        match proj {
            Projection::Global(def) => {
                out.clear();
                let _ = write!(out, "global#{}", def.0);
            }
            Projection::Deref => out.push_str(".*"),
            Projection::Field(index) => {
                let _ = write!(out, ".field{index}");
            }
            Projection::Index(index) => {
                let _ = write!(out, "[{}]", fmt_operand(index));
            }
        }
    }
    out
}

fn fmt_ty(tcx: &TyCtxt, ty: TyId) -> String {
    match tcx.get(ty) {
        Ty::Void => "void".to_string(),
        Ty::Int { signed, rank } => fmt_int(*signed, *rank).to_string(),
        Ty::Float(kind) => fmt_float(*kind).to_string(),
        Ty::Complex(kind) => format!("_Complex {}", fmt_float(*kind)),
        Ty::Ptr(q) => format!("{}*", fmt_qual(tcx, *q)),
        Ty::Array { elem, len, is_vla } => {
            let len = if *is_vla {
                "vla".to_string()
            } else {
                len.map_or_else(|| "?".to_string(), |n| n.to_string())
            };
            format!("{}[{len}]", fmt_qual(tcx, *elem))
        }
        Ty::Func { ret, params, variadic, proto } => {
            let mut parts = params.iter().map(|p| fmt_ty(tcx, *p)).collect::<Vec<_>>();
            if *variadic {
                parts.push("...".to_string());
            }
            if !*proto && parts.is_empty() {
                parts.push("<unspecified>".to_string());
            }
            format!("fn({}) -> {}", parts.join(", "), fmt_ty(tcx, *ret))
        }
        Ty::Record(def) => format!("record#{}", def.0),
        Ty::Enum(def) => format!("enum#{}", def.0),
        Ty::BuiltinVaList => "__builtin_va_list".to_string(),
        Ty::Error => "<error>".to_string(),
    }
}

fn fmt_qual(tcx: &TyCtxt, q: Qual) -> String {
    let mut quals = Vec::new();
    if q.is_const {
        quals.push("const");
    }
    if q.is_volatile {
        quals.push("volatile");
    }
    if q.is_restrict {
        quals.push("restrict");
    }
    let base = fmt_ty(tcx, q.ty);
    if quals.is_empty() {
        base
    } else {
        format!("{} {base}", quals.join(" "))
    }
}

fn fmt_int(signed: bool, rank: IntRank) -> &'static str {
    match (signed, rank) {
        (false, IntRank::Bool) => "_Bool",
        (true, IntRank::Char) => "char",
        (false, IntRank::Char) => "unsigned char",
        (true, IntRank::Short) => "short",
        (false, IntRank::Short) => "unsigned short",
        (true, IntRank::Int) => "int",
        (false, IntRank::Int) => "unsigned int",
        (true, IntRank::Long) => "long",
        (false, IntRank::Long) => "unsigned long",
        (true, IntRank::LongLong) => "long long",
        (false, IntRank::LongLong) => "unsigned long long",
        (true, IntRank::Bool) => "_Bool",
    }
}

fn fmt_float(kind: FloatKind) -> &'static str {
    match kind {
        FloatKind::F32 => "float",
        FloatKind::F64 => "double",
        FloatKind::F80 => "long double",
    }
}

fn fmt_unop(op: UnOp) -> &'static str {
    match op {
        UnOp::Neg => "Neg",
        UnOp::FNeg => "FNeg",
        UnOp::BitNot => "BitNot",
        UnOp::LogNot => "LogNot",
    }
}

fn fmt_local(local: Local) -> String {
    format!("_{}", local.0)
}

fn fmt_bb(bb: BasicBlockId) -> String {
    format!("bb{}", bb.0)
}
