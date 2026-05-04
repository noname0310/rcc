//! Final typed-HIR invariant checks before CFG/codegen.

use rcc_hir::{
    Body, DefId, DefKind, GlobalInit, GlobalInitValue, HirCrate, HirExprId, HirExprKind, Local, Ty,
    TyCtxt, TyId,
};
use rcc_session::Session;
use rcc_span::{Span, DUMMY_SP};

/// Verify that a successful type-checking pass left no type placeholders
/// in the HIR boundary consumed by CFG and LLVM codegen.
///
/// This is a phase-boundary gate, not a user-facing semantic checker. If
/// earlier phases already emitted errors, the verifier stays quiet and
/// returns `false`; the driver will stop before CFG for those diagnostics.
/// On an otherwise clean session, any remaining [`Ty::Error`] or unresolved
/// HIR placeholder is reported with `E0088` at the source span that still
/// carries the bad state.
pub fn verify_typed_hir(session: &mut Session, tcx: &TyCtxt, hir: &HirCrate) -> bool {
    if session.handler.has_errors() {
        return false;
    }

    let mut cx = VerifyCx { session, tcx, ok: true };
    cx.verify_defs(hir);
    cx.verify_global_initializers(hir);
    cx.verify_bodies("function", &hir.bodies);
    cx.verify_bodies("global initializer", &hir.global_init_bodies);
    cx.ok
}

struct VerifyCx<'a> {
    session: &'a mut Session,
    tcx: &'a TyCtxt,
    ok: bool,
}

impl VerifyCx<'_> {
    fn verify_defs(&mut self, hir: &HirCrate) {
        for (def_id, def) in hir.defs.iter_enumerated() {
            match &def.kind {
                DefKind::Function { ty, .. } => {
                    self.verify_ty(def.span, *ty, format_args!("function def#{} type", def_id.0));
                }
                DefKind::Global { ty, .. } => {
                    self.verify_ty(def.span, *ty, format_args!("global def#{} type", def_id.0));
                }
                DefKind::Typedef(ty) => {
                    self.verify_ty(def.span, *ty, format_args!("typedef def#{} type", def_id.0));
                }
                DefKind::Record { fields, .. } => {
                    for (idx, field) in fields.iter().enumerate() {
                        self.verify_ty(
                            field.span,
                            field.ty,
                            format_args!("record def#{} field #{idx} type", def_id.0),
                        );
                    }
                }
                DefKind::Enum { repr, .. } => {
                    self.verify_ty(
                        def.span,
                        *repr,
                        format_args!("enum def#{} repr type", def_id.0),
                    );
                }
                DefKind::Enumerator { ty, .. } => {
                    self.verify_ty(def.span, *ty, format_args!("enumerator def#{} type", def_id.0));
                }
            }
        }
    }

    fn verify_global_initializers(&mut self, hir: &HirCrate) {
        for (def_id, def) in hir.defs.iter_enumerated() {
            let DefKind::Global { init: Some(init), .. } = &def.kind else {
                continue;
            };
            self.verify_init(def_id, init);
        }
    }

    fn verify_init(&mut self, def_id: DefId, init: &GlobalInit) {
        self.verify_ty(
            init.entries.first().map_or(DUMMY_SP, |entry| entry.span),
            init.ty,
            format_args!("global def#{} initializer object type", def_id.0),
        );
        for (idx, entry) in init.entries.iter().enumerate() {
            self.verify_ty(
                entry.span,
                entry.ty,
                format_args!("global def#{} initializer entry #{idx} type", def_id.0),
            );
            if matches!(entry.value, GlobalInitValue::Error) {
                self.report(
                    entry.span,
                    format_args!("global def#{} initializer entry #{idx}", def_id.0),
                    "initializer leaf still has an error value",
                );
            }
        }
    }

    fn verify_bodies(&mut self, kind: &str, bodies: &rcc_data_structures::FxHashMap<DefId, Body>) {
        for (def_id, body) in bodies {
            self.verify_body(*def_id, kind, body);
        }
    }

    fn verify_body(&mut self, def_id: DefId, kind: &str, body: &Body) {
        for (local, decl) in body.locals.iter_enumerated() {
            self.verify_ty(
                decl.span,
                decl.ty,
                format_args!("{kind} def#{} local #{} type", def_id.0, local.0),
            );
            if let Some(vla_len) = decl.vla_len {
                self.verify_expr_id(body, def_id, kind, decl.span, vla_len, local);
            }
        }

        for (expr_id, expr) in body.exprs.iter_enumerated() {
            self.verify_ty(
                expr.span,
                expr.ty,
                format_args!("{kind} def#{} expr #{} type", def_id.0, expr_id.0),
            );
            match &expr.kind {
                HirExprKind::UnresolvedField { field_span, .. } => {
                    self.report(
                        *field_span,
                        format_args!("{kind} def#{} expr #{}", def_id.0, expr_id.0),
                        "unresolved member-access placeholder survived type checking",
                    );
                }
                HirExprKind::Cast { to, .. } => {
                    self.verify_ty(
                        expr.span,
                        *to,
                        format_args!("{kind} def#{} cast destination type", def_id.0),
                    );
                }
                HirExprKind::SizeofType(ty) => {
                    self.verify_ty(
                        expr.span,
                        *ty,
                        format_args!("{kind} def#{} sizeof(type-name) type", def_id.0),
                    );
                }
                HirExprKind::CompoundLiteral { ty, local, .. } => {
                    self.verify_ty(
                        expr.span,
                        *ty,
                        format_args!("{kind} def#{} compound-literal type", def_id.0),
                    );
                    self.verify_local(body, def_id, kind, expr.span, *local);
                }
                _ => {}
            }
        }
    }

    fn verify_expr_id(
        &mut self,
        body: &Body,
        def_id: DefId,
        kind: &str,
        span: Span,
        expr: HirExprId,
        owner: Local,
    ) {
        if (expr.0 as usize) >= body.exprs.len() {
            self.report(
                span,
                format_args!("{kind} def#{} local #{} VLA length", def_id.0, owner.0),
                "references a missing expression",
            );
        }
    }

    fn verify_local(&mut self, body: &Body, def_id: DefId, kind: &str, span: Span, local: Local) {
        if (local.0 as usize) >= body.locals.len() {
            self.report(
                span,
                format_args!("{kind} def#{} compound literal", def_id.0),
                "references a missing backing local",
            );
        }
    }

    fn verify_ty(&mut self, span: Span, ty: TyId, context: std::fmt::Arguments<'_>) {
        if ty_contains_error(self.tcx, ty) {
            self.report(span, context, "contains Ty::Error");
        }
    }

    fn report(&mut self, span: Span, context: std::fmt::Arguments<'_>, message: &'static str) {
        self.ok = false;
        self.session
            .handler
            .struct_err(span, format!("typed HIR invariant violation: {context} {message}"))
            .code(rcc_errors::codes::E0088)
            .emit();
    }
}

fn ty_contains_error(tcx: &TyCtxt, ty: TyId) -> bool {
    if ty == tcx.error {
        return true;
    }
    match tcx.get(ty) {
        Ty::Ptr(q) => ty_contains_error(tcx, q.ty),
        Ty::Array { elem, .. } => ty_contains_error(tcx, elem.ty),
        Ty::Vector { elem, .. } => ty_contains_error(tcx, *elem),
        Ty::Func { ret, params, .. } => {
            ty_contains_error(tcx, *ret)
                || params.iter().any(|param| ty_contains_error(tcx, *param))
        }
        Ty::Error => true,
        Ty::BuiltinVaList => false,
        Ty::Void | Ty::Int { .. } | Ty::Float(_) | Ty::Complex(_) | Ty::Record(_) | Ty::Enum(_) => {
            false
        }
    }
}
