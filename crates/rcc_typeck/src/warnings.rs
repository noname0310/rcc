//! Warning passes that need typed HIR.

use std::collections::BTreeSet;

use rcc_errors::codes;
use rcc_hir::{
    Body, DefId, DefKind, HirCrate, HirExprId, HirExprKind, HirStmtId, HirStmtKind, Local, Ty,
    TyCtxt,
};
use rcc_session::Session;

/// Run typed-HIR warning checks.
pub fn check_warnings(session: &mut Session, tcx: &TyCtxt, hir: &HirCrate) {
    warn_unused_functions(session, tcx, hir);
    for body in hir.bodies.values() {
        warn_unused_variables_in_body(session, tcx, body);
    }
}

fn warn_unused_functions(session: &mut Session, tcx: &TyCtxt, hir: &HirCrate) {
    let referenced = collect_referenced_defs(tcx, hir);
    for (def_id, def) in hir.defs.iter_enumerated() {
        let DefKind::Function { has_body: true, is_static: true, .. } = def.kind else {
            continue;
        };
        if referenced.contains(&def_id) {
            continue;
        }
        let name = session.interner.get(def.name);
        session
            .handler
            .struct_warn(def.span, format!("unused function `{name}` [-Wunused-function]"))
            .code(codes::W0027)
            .help("remove the function, reference it, or suppress with `-Wno-unused-function`")
            .emit();
    }
}

fn collect_referenced_defs(tcx: &TyCtxt, hir: &HirCrate) -> BTreeSet<DefId> {
    let mut usage = LocalUsage::new();
    for body in hir.bodies.values().chain(hir.global_init_bodies.values()) {
        UsageWalker { body, tcx, usage: &mut usage }.visit_reachable_body();
    }
    usage.def_refs
}

fn warn_unused_variables_in_body(session: &mut Session, tcx: &TyCtxt, body: &Body) {
    let mut usage = LocalUsage::new();
    UsageWalker { body, tcx, usage: &mut usage }.visit_reachable_body();

    for (local, decl) in body.locals.iter_enumerated() {
        if decl.is_param || decl.name.is_none() || decl.quals.is_volatile {
            continue;
        }
        if !usage.declared.contains(&local) || usage.read.contains(&local) {
            continue;
        }
        let name = session.interner.get(decl.name.expect("checked above"));
        session
            .handler
            .struct_warn(decl.span, format!("unused variable `{name}` [-Wunused-variable]"))
            .code(codes::W0026)
            .help("remove the variable, read it, or suppress with `-Wno-unused-variable`")
            .emit();
    }
}

#[derive(Debug)]
struct LocalUsage {
    declared: BTreeSet<Local>,
    read: BTreeSet<Local>,
    def_refs: BTreeSet<DefId>,
}

impl LocalUsage {
    fn new() -> Self {
        Self { declared: BTreeSet::new(), read: BTreeSet::new(), def_refs: BTreeSet::new() }
    }
}

struct UsageWalker<'a> {
    body: &'a Body,
    tcx: &'a TyCtxt,
    usage: &'a mut LocalUsage,
}

impl UsageWalker<'_> {
    fn visit_reachable_body(&mut self) {
        if let Some(root) = self.body.root {
            self.visit_stmt(root);
        } else {
            for (expr, _) in self.body.exprs.iter_enumerated() {
                self.visit_value_expr(expr);
            }
        }
    }

    fn visit_stmt(&mut self, stmt: HirStmtId) {
        match &self.body.stmts[stmt].kind {
            HirStmtKind::Block(stmts) => {
                for stmt in stmts {
                    self.visit_stmt(*stmt);
                }
            }
            HirStmtKind::Expr(expr) => self.visit_value_expr(*expr),
            HirStmtKind::If { cond, then_branch, else_branch } => {
                self.visit_value_expr(*cond);
                self.visit_stmt(*then_branch);
                if let Some(else_branch) = else_branch {
                    self.visit_stmt(*else_branch);
                }
            }
            HirStmtKind::While { cond, body } => {
                self.visit_value_expr(*cond);
                self.visit_stmt(*body);
            }
            HirStmtKind::DoWhile { body, cond } => {
                self.visit_stmt(*body);
                self.visit_value_expr(*cond);
            }
            HirStmtKind::For { init, cond, step, body } => {
                if let Some(init) = init {
                    self.visit_stmt(*init);
                }
                if let Some(cond) = cond {
                    self.visit_value_expr(*cond);
                }
                if let Some(step) = step {
                    self.visit_value_expr(*step);
                }
                self.visit_stmt(*body);
            }
            HirStmtKind::Switch { cond, body, .. } => {
                self.visit_value_expr(*cond);
                self.visit_stmt(*body);
            }
            HirStmtKind::Label { body, .. }
            | HirStmtKind::Case { body, .. }
            | HirStmtKind::Default { body } => self.visit_stmt(*body),
            HirStmtKind::Goto(_)
            | HirStmtKind::Break
            | HirStmtKind::Continue
            | HirStmtKind::Null => {}
            HirStmtKind::GotoComputed(expr) => self.visit_value_expr(*expr),
            HirStmtKind::Return(expr) => {
                if let Some(expr) = expr {
                    self.visit_value_expr(*expr);
                }
            }
            HirStmtKind::LocalDecl { local, init } => {
                self.usage.declared.insert(*local);
                if let Some(init) = init {
                    self.visit_value_expr(*init);
                }
            }
        }
    }

    fn visit_value_expr(&mut self, expr: HirExprId) {
        match &self.body.exprs[expr].kind {
            HirExprKind::IntLiteral { .. }
            | HirExprKind::IntConst(_)
            | HirExprKind::FloatConst(_)
            | HirExprKind::StringRef(_)
            | HirExprKind::SizeofType(_)
            | HirExprKind::AlignofType(_)
            | HirExprKind::LabelAddr(_)
            | HirExprKind::BuiltinVaArea => {}
            HirExprKind::DefRef(def) => {
                self.usage.def_refs.insert(*def);
            }
            HirExprKind::LocalRef(local) => {
                self.usage.read.insert(*local);
            }
            HirExprKind::Binary { lhs, rhs, .. } => {
                self.visit_value_expr(*lhs);
                self.visit_value_expr(*rhs);
            }
            HirExprKind::Unary { operand, .. }
            | HirExprKind::Cast { operand, .. }
            | HirExprKind::SizeofExpr(operand)
            | HirExprKind::AlignofExpr(operand) => self.visit_value_expr(*operand),
            HirExprKind::Call { callee, args } => {
                self.visit_value_expr(*callee);
                for arg in args {
                    self.visit_value_expr(*arg);
                }
            }
            HirExprKind::StmtExpr { stmts, result } => {
                for stmt in stmts {
                    self.visit_stmt(*stmt);
                }
                if let Some(result) = result {
                    self.visit_value_expr(*result);
                }
            }
            HirExprKind::UnresolvedField { base, .. } | HirExprKind::Field { base, .. } => {
                self.visit_lvalue_read(*base);
            }
            HirExprKind::Index { base, index } => {
                self.visit_lvalue_read(*base);
                self.visit_value_expr(*index);
            }
            HirExprKind::Convert { operand, kind }
                if *kind == rcc_hir::ConvertKind::LvalueToRvalue =>
            {
                self.visit_lvalue_read(*operand);
            }
            HirExprKind::Convert { operand, .. } => self.visit_value_expr(*operand),
            HirExprKind::CompoundLiteral { init_stmts, .. } => {
                for stmt in init_stmts {
                    self.visit_stmt(*stmt);
                }
            }
            HirExprKind::VectorInit { lanes, .. } => {
                for lane in lanes {
                    self.visit_value_expr(*lane);
                }
            }
            HirExprKind::AddressOf(operand) => self.visit_lvalue_address_use(*operand),
            HirExprKind::Deref(operand) => self.visit_value_expr(*operand),
            HirExprKind::Cond { cond, then_expr, else_expr } => {
                self.visit_value_expr(*cond);
                self.visit_value_expr(*then_expr);
                self.visit_value_expr(*else_expr);
            }
            HirExprKind::OmittedCond { cond, else_expr } => {
                self.visit_value_expr(*cond);
                self.visit_value_expr(*else_expr);
            }
            HirExprKind::Comma { lhs, rhs } => {
                self.visit_value_expr(*lhs);
                self.visit_value_expr(*rhs);
            }
            HirExprKind::Assign { lhs, rhs } => {
                self.visit_lvalue_write(*lhs);
                self.visit_value_expr(*rhs);
            }
            HirExprKind::BuiltinVaArg { ap, .. } => self.visit_value_expr(*ap),
            HirExprKind::BuiltinVaStart { ap, last_param } => {
                self.visit_lvalue_address_use(*ap);
                self.visit_value_expr(*last_param);
            }
            HirExprKind::BuiltinVaEnd { ap } => self.visit_lvalue_address_use(*ap),
            HirExprKind::BuiltinVaCopy { dst, src } => {
                self.visit_lvalue_address_use(*dst);
                self.visit_value_expr(*src);
            }
            HirExprKind::BuiltinExpect { value, expected } => {
                self.visit_value_expr(*value);
                self.visit_value_expr(*expected);
            }
            HirExprKind::BuiltinOverflow { lhs, rhs, dst, .. } => {
                self.visit_value_expr(*lhs);
                self.visit_value_expr(*rhs);
                self.visit_value_expr(*dst);
            }
            HirExprKind::BuiltinOverflowP { lhs, rhs, probe, .. } => {
                self.visit_value_expr(*lhs);
                self.visit_value_expr(*rhs);
                self.visit_value_expr(*probe);
            }
        }
    }

    fn visit_lvalue_read(&mut self, expr: HirExprId) {
        match &self.body.exprs[expr].kind {
            HirExprKind::LocalRef(local) => {
                self.usage.read.insert(*local);
            }
            HirExprKind::Field { base, .. } | HirExprKind::UnresolvedField { base, .. } => {
                self.visit_lvalue_read(*base);
            }
            HirExprKind::Index { base, index } => {
                self.visit_lvalue_read(*base);
                self.visit_value_expr(*index);
            }
            HirExprKind::Deref(ptr) => self.visit_value_expr(*ptr),
            HirExprKind::Convert { operand, .. } => self.visit_lvalue_read(*operand),
            HirExprKind::Comma { lhs, rhs } => {
                self.visit_value_expr(*lhs);
                self.visit_lvalue_read(*rhs);
            }
            HirExprKind::Cond { cond, then_expr, else_expr } => {
                self.visit_value_expr(*cond);
                self.visit_lvalue_read(*then_expr);
                self.visit_lvalue_read(*else_expr);
            }
            HirExprKind::OmittedCond { cond, else_expr } => {
                self.visit_value_expr(*cond);
                self.visit_lvalue_read(*else_expr);
            }
            _ => self.visit_value_expr(expr),
        }
    }

    fn visit_lvalue_address_use(&mut self, expr: HirExprId) {
        match &self.body.exprs[expr].kind {
            HirExprKind::LocalRef(local) => {
                self.usage.read.insert(*local);
            }
            HirExprKind::Field { base, .. } | HirExprKind::UnresolvedField { base, .. } => {
                self.visit_lvalue_address_use(*base);
            }
            HirExprKind::Index { base, index } => {
                self.visit_lvalue_address_use(*base);
                self.visit_value_expr(*index);
            }
            HirExprKind::Deref(ptr) => self.visit_value_expr(*ptr),
            HirExprKind::Convert { operand, .. } => self.visit_lvalue_address_use(*operand),
            _ => self.visit_value_expr(expr),
        }
    }

    fn visit_lvalue_write(&mut self, expr: HirExprId) {
        match &self.body.exprs[expr].kind {
            HirExprKind::LocalRef(_) => {}
            HirExprKind::Field { base, .. } | HirExprKind::UnresolvedField { base, .. } => {
                self.visit_lvalue_write(*base);
            }
            HirExprKind::Index { base, index } => {
                self.visit_lvalue_index_base_for_write(*base);
                self.visit_value_expr(*index);
            }
            HirExprKind::Deref(ptr) => self.visit_value_expr(*ptr),
            HirExprKind::Convert { operand, .. } => self.visit_lvalue_write(*operand),
            HirExprKind::Comma { lhs, rhs } => {
                self.visit_value_expr(*lhs);
                self.visit_lvalue_write(*rhs);
            }
            HirExprKind::Cond { cond, then_expr, else_expr } => {
                self.visit_value_expr(*cond);
                self.visit_lvalue_write(*then_expr);
                self.visit_lvalue_write(*else_expr);
            }
            HirExprKind::OmittedCond { cond, else_expr } => {
                self.visit_value_expr(*cond);
                self.visit_lvalue_write(*else_expr);
            }
            _ => self.visit_value_expr(expr),
        }
    }

    fn visit_lvalue_index_base_for_write(&mut self, expr: HirExprId) {
        if matches!(self.tcx.get(self.body.exprs[expr].ty), Ty::Array { .. }) {
            self.visit_lvalue_write(expr);
        } else {
            self.visit_value_expr(expr);
        }
    }
}

#[cfg(test)]
mod tests {
    use rcc_errors::{codes, Level};
    use rcc_hir::{
        Body, Def, DefId, DefKind, HirCrate, HirExpr, HirExprId, HirExprKind, HirStmt, HirStmtId,
        HirStmtKind, Local, LocalDecl, ObjectQuals, Ty, TyCtxt, ValueCat,
    };
    use rcc_session::{Session, WarningConfig};
    use rcc_span::DUMMY_SP;

    use super::{warn_unused_functions, warn_unused_variables_in_body};

    fn enable_wall(session: &mut Session) {
        let mut config = WarningConfig::default();
        config.enable_wall();
        session.opts.warning_config = config.clone();
        session.handler.set_warning_config(config);
    }

    fn push_expr(body: &mut Body, ty: rcc_hir::TyId, kind: HirExprKind) -> HirExprId {
        let id = body.exprs.push(HirExpr {
            id: HirExprId(0),
            ty,
            value_cat: ValueCat::RValue,
            span: DUMMY_SP,
            kind,
        });
        body.exprs[id].id = id;
        id
    }

    fn push_local_ref(body: &mut Body, local: Local, ty: rcc_hir::TyId) -> HirExprId {
        let id = push_expr(body, ty, HirExprKind::LocalRef(local));
        body.exprs[id].value_cat = ValueCat::LValue;
        id
    }

    fn push_stmt(body: &mut Body, kind: HirStmtKind) -> HirStmtId {
        let id = body.stmts.push(HirStmt { id: HirStmtId(0), span: DUMMY_SP, kind });
        body.stmts[id].id = id;
        id
    }

    fn one_local_body(session: &mut Session, volatile: bool) -> (Body, TyCtxt, Local) {
        let tcx = TyCtxt::new();
        let mut body = Body::default();
        body.locals.push(LocalDecl {
            name: None,
            ty: tcx.int,
            quals: ObjectQuals::none(),
            vla_len: None,
            is_param: false,
            span: DUMMY_SP,
        });
        let x = session.interner.intern("x");
        let local = body.locals.push(LocalDecl {
            name: Some(x),
            ty: tcx.int,
            quals: ObjectQuals { is_const: false, is_volatile: volatile, is_restrict: false },
            vla_len: None,
            is_param: false,
            span: DUMMY_SP,
        });
        let decl = push_stmt(&mut body, HirStmtKind::LocalDecl { local, init: None });
        body.root = Some(push_stmt(&mut body, HirStmtKind::Block(vec![decl])));
        (body, tcx, local)
    }

    fn push_function_def(
        hir: &mut HirCrate,
        session: &mut Session,
        tcx: &mut TyCtxt,
        name: &str,
        is_static: bool,
        has_body: bool,
    ) -> DefId {
        let fn_ty =
            tcx.intern(Ty::Func { ret: tcx.int, params: Vec::new(), variadic: false, proto: true });
        let name = session.interner.intern(name);
        let id = hir.defs.push(Def {
            id: DefId(0),
            name,
            span: DUMMY_SP,
            kind: DefKind::Function {
                ty: fn_ty,
                has_body,
                is_static,
                is_inline: false,
                is_extern_inline: false,
                no_instrument_function: false,
                variadic: false,
            },
        });
        hir.defs[id].id = id;
        if has_body {
            let mut body = Body::default();
            body.root = Some(push_stmt(&mut body, HirStmtKind::Block(Vec::new())));
            hir.bodies.insert(id, body);
        }
        id
    }

    #[test]
    fn warns_for_declared_unread_local() {
        let (mut session, cap) = Session::for_test();
        enable_wall(&mut session);
        let (body, tcx, _) = one_local_body(&mut session, false);

        warn_unused_variables_in_body(&mut session, &tcx, &body);

        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].level, Level::Warning);
        assert_eq!(diags[0].code, Some(codes::W0026));
        assert!(diags[0].message.contains("[-Wunused-variable]"));
    }

    #[test]
    fn read_local_suppresses_warning() {
        let (mut session, cap) = Session::for_test();
        enable_wall(&mut session);
        let (mut body, tcx, local) = one_local_body(&mut session, false);
        let local_ref = push_local_ref(&mut body, local, tcx.int);
        let read = push_expr(
            &mut body,
            tcx.int,
            HirExprKind::Convert { operand: local_ref, kind: rcc_hir::ConvertKind::LvalueToRvalue },
        );
        let ret = push_stmt(&mut body, HirStmtKind::Return(Some(read)));
        let HirStmtKind::Block(stmts) = &mut body.stmts[body.root.unwrap()].kind else {
            panic!("root is block");
        };
        stmts.push(ret);

        warn_unused_variables_in_body(&mut session, &tcx, &body);

        assert!(cap.diagnostics().is_empty());
    }

    #[test]
    fn assignment_only_remains_unused() {
        let (mut session, cap) = Session::for_test();
        enable_wall(&mut session);
        let (mut body, tcx, local) = one_local_body(&mut session, false);
        let lhs = push_local_ref(&mut body, local, tcx.int);
        let rhs = push_expr(&mut body, tcx.int, HirExprKind::IntConst(1));
        let assign = push_expr(&mut body, tcx.int, HirExprKind::Assign { lhs, rhs });
        let stmt = push_stmt(&mut body, HirStmtKind::Expr(assign));
        let HirStmtKind::Block(stmts) = &mut body.stmts[body.root.unwrap()].kind else {
            panic!("root is block");
        };
        stmts.push(stmt);

        warn_unused_variables_in_body(&mut session, &tcx, &body);

        assert_eq!(cap.diagnostics().len(), 1);
    }

    #[test]
    fn volatile_local_is_not_warned() {
        let (mut session, cap) = Session::for_test();
        enable_wall(&mut session);
        let (body, tcx, _) = one_local_body(&mut session, true);

        warn_unused_variables_in_body(&mut session, &tcx, &body);

        assert!(cap.diagnostics().is_empty());
    }

    #[test]
    fn warns_for_unreferenced_static_function() {
        let (mut session, cap) = Session::for_test();
        enable_wall(&mut session);
        let mut tcx = TyCtxt::new();
        let mut hir = HirCrate::default();
        push_function_def(&mut hir, &mut session, &mut tcx, "helper", true, true);
        push_function_def(&mut hir, &mut session, &mut tcx, "main", false, true);

        warn_unused_functions(&mut session, &tcx, &hir);

        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].level, Level::Warning);
        assert_eq!(diags[0].code, Some(codes::W0027));
        assert!(diags[0].message.contains("[-Wunused-function]"));
    }

    #[test]
    fn def_ref_suppresses_unused_function_warning() {
        let (mut session, cap) = Session::for_test();
        enable_wall(&mut session);
        let mut tcx = TyCtxt::new();
        let mut hir = HirCrate::default();
        let helper = push_function_def(&mut hir, &mut session, &mut tcx, "helper", true, true);
        let main = push_function_def(&mut hir, &mut session, &mut tcx, "main", false, true);
        let helper_ty = match hir.defs[helper].kind {
            DefKind::Function { ty, .. } => ty,
            _ => unreachable!(),
        };
        let body = hir.bodies.get_mut(&main).expect("main body exists");
        let helper_ref = push_expr(body, helper_ty, HirExprKind::DefRef(helper));
        let stmt = push_stmt(body, HirStmtKind::Expr(helper_ref));
        let HirStmtKind::Block(stmts) = &mut body.stmts[body.root.unwrap()].kind else {
            panic!("root is block");
        };
        stmts.push(stmt);

        warn_unused_functions(&mut session, &tcx, &hir);

        assert!(cap.diagnostics().is_empty());
    }
}
