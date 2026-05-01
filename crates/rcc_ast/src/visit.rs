//! Immutable AST visitor. Default `visit_*` methods walk children;
//! override any you care about.

use crate::{
    Block, BlockItem, Decl, Declarator, EnumSpec, Expr, ExprKind, ExternalDecl, FieldDecl,
    FunctionDef, InitDeclarator, Initializer, OffsetofDesignator, ParamDecl, RecordSpec, Stmt,
    StmtKind, TranslationUnit, TypeName,
};

/// Walk the AST read-only.
pub trait Visitor: Sized {
    /// Override to visit a translation unit.
    fn visit_translation_unit(&mut self, tu: &TranslationUnit) {
        walk_translation_unit(self, tu);
    }
    /// Override to visit an external declaration.
    fn visit_external_decl(&mut self, d: &ExternalDecl) {
        walk_external_decl(self, d);
    }
    /// Override to visit a declaration.
    fn visit_decl(&mut self, d: &Decl) {
        walk_decl(self, d);
    }
    /// Override to visit a function definition.
    fn visit_function_def(&mut self, f: &FunctionDef) {
        walk_function_def(self, f);
    }
    /// Override to visit an init-declarator.
    fn visit_init_declarator(&mut self, i: &InitDeclarator) {
        walk_init_declarator(self, i);
    }
    /// Override to visit a declarator.
    fn visit_declarator(&mut self, _d: &Declarator) {}
    /// Override to visit a type name.
    fn visit_type_name(&mut self, _t: &TypeName) {}
    /// Override to visit a record specifier.
    fn visit_record(&mut self, r: &RecordSpec) {
        if let Some(fs) = &r.fields {
            for f in fs {
                self.visit_field(f);
            }
        }
    }
    /// Override to visit an enum specifier.
    fn visit_enum(&mut self, _e: &EnumSpec) {}
    /// Override to visit a struct/union field.
    fn visit_field(&mut self, _f: &FieldDecl) {}
    /// Override to visit a parameter.
    fn visit_param(&mut self, _p: &ParamDecl) {}
    /// Override to visit an initializer.
    fn visit_initializer(&mut self, _i: &Initializer) {}
    /// Override to visit a block.
    fn visit_block(&mut self, b: &Block) {
        walk_block(self, b);
    }
    /// Override to visit a block item.
    fn visit_block_item(&mut self, b: &BlockItem) {
        walk_block_item(self, b);
    }
    /// Override to visit a statement.
    fn visit_stmt(&mut self, s: &Stmt) {
        walk_stmt(self, s);
    }
    /// Override to visit an expression.
    fn visit_expr(&mut self, e: &Expr) {
        walk_expr(self, e);
    }
}

/// Default descent for `TranslationUnit`.
pub fn walk_translation_unit<V: Visitor>(v: &mut V, tu: &TranslationUnit) {
    for d in &tu.decls {
        v.visit_external_decl(d);
    }
}

/// Default descent for `ExternalDecl`.
pub fn walk_external_decl<V: Visitor>(v: &mut V, d: &ExternalDecl) {
    match d {
        ExternalDecl::Function(f) => v.visit_function_def(f),
        ExternalDecl::Decl(d) => v.visit_decl(d),
    }
}

/// Default descent for `Decl`.
pub fn walk_decl<V: Visitor>(v: &mut V, d: &Decl) {
    for id in &d.inits {
        v.visit_init_declarator(id);
    }
}

/// Default descent for `FunctionDef`.
pub fn walk_function_def<V: Visitor>(v: &mut V, f: &FunctionDef) {
    v.visit_declarator(&f.declarator);
    for d in &f.kr_decls {
        v.visit_decl(d);
    }
    v.visit_block(&f.body);
}

/// Default descent for `InitDeclarator`.
pub fn walk_init_declarator<V: Visitor>(v: &mut V, i: &InitDeclarator) {
    v.visit_declarator(&i.declarator);
    if let Some(init) = &i.init {
        v.visit_initializer(init);
    }
}

/// Default descent for `Block`.
pub fn walk_block<V: Visitor>(v: &mut V, b: &Block) {
    for item in &b.items {
        v.visit_block_item(item);
    }
}

/// Default descent for `BlockItem`.
pub fn walk_block_item<V: Visitor>(v: &mut V, b: &BlockItem) {
    match b {
        BlockItem::Decl(d) => v.visit_decl(d),
        BlockItem::Stmt(s) => v.visit_stmt(s),
    }
}

/// Default descent for `Stmt`.
pub fn walk_stmt<V: Visitor>(v: &mut V, s: &Stmt) {
    match &s.kind {
        StmtKind::Expr(e) => {
            if let Some(e) = e {
                v.visit_expr(e);
            }
        }
        StmtKind::Compound(b) => v.visit_block(b),
        StmtKind::If { cond, then_branch, else_branch } => {
            v.visit_expr(cond);
            v.visit_stmt(then_branch);
            if let Some(e) = else_branch {
                v.visit_stmt(e);
            }
        }
        StmtKind::While { cond, body } | StmtKind::Switch { cond, body } => {
            v.visit_expr(cond);
            v.visit_stmt(body);
        }
        StmtKind::DoWhile { body, cond } => {
            v.visit_stmt(body);
            v.visit_expr(cond);
        }
        StmtKind::For { init, cond, step, body } => {
            if let Some(i) = init {
                v.visit_block_item(i);
            }
            if let Some(c) = cond {
                v.visit_expr(c);
            }
            if let Some(st) = step {
                v.visit_expr(st);
            }
            v.visit_stmt(body);
        }
        StmtKind::Case { value, body } => {
            v.visit_expr(value);
            v.visit_stmt(body);
        }
        StmtKind::Default { body } | StmtKind::Label { body, .. } => v.visit_stmt(body),
        StmtKind::Return(e) => {
            if let Some(e) = e {
                v.visit_expr(e);
            }
        }
        StmtKind::Goto(_) | StmtKind::Break | StmtKind::Continue | StmtKind::Null => {}
    }
}

/// Default descent for `Expr`.
pub fn walk_expr<V: Visitor>(v: &mut V, e: &Expr) {
    match &e.kind {
        ExprKind::Binary { lhs, rhs, .. }
        | ExprKind::Assign { lhs, rhs, .. }
        | ExprKind::Comma { lhs, rhs } => {
            v.visit_expr(lhs);
            v.visit_expr(rhs);
        }
        ExprKind::Unary { operand, .. }
        | ExprKind::Member { base: operand, .. }
        | ExprKind::Arrow { base: operand, .. }
        | ExprKind::Paren(operand)
        | ExprKind::SizeofExpr(operand) => v.visit_expr(operand),
        ExprKind::Cond { cond, then_expr, else_expr } => {
            v.visit_expr(cond);
            v.visit_expr(then_expr);
            v.visit_expr(else_expr);
        }
        ExprKind::Call { callee, args } => {
            v.visit_expr(callee);
            for a in args {
                v.visit_expr(a);
            }
        }
        ExprKind::BuiltinOffsetof { ty, designators } => {
            v.visit_type_name(ty);
            for designator in designators {
                if let OffsetofDesignator::Index(index) = designator {
                    v.visit_expr(index);
                }
            }
        }
        ExprKind::BuiltinTypesCompatible { lhs, rhs } => {
            v.visit_type_name(lhs);
            v.visit_type_name(rhs);
        }
        ExprKind::Index { base, index } => {
            v.visit_expr(base);
            v.visit_expr(index);
        }
        ExprKind::Cast { ty, expr } => {
            v.visit_type_name(ty);
            v.visit_expr(expr);
        }
        ExprKind::SizeofType(t) => v.visit_type_name(t),
        ExprKind::CompoundLiteral { ty, init } => {
            v.visit_type_name(ty);
            v.visit_initializer(init.as_ref());
        }
        ExprKind::Ident(_)
        | ExprKind::IntLit(_)
        | ExprKind::FloatLit(_)
        | ExprKind::CharLit(_)
        | ExprKind::StringLit(_) => {}
    }
}
