//! `rcc_hir_lower`: AST -> HIR lowering.
//!
//! Analogous to `rustc_ast_lowering`. Responsibilities:
//!
//! 1. Resolve identifiers against three *separate* C name spaces
//!    (ordinary / tag / label).
//! 2. Flatten declarators (`int (*fp[3])(int,int)`) into `Ty`.
//! 3. Expand `typedef` references.
//! 4. Assign `DefId`s and `HirId`s.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use rcc_ast::{ExternalDecl, StorageClass, TranslationUnit, TypeSpec};
use rcc_data_structures::FxHashMap;
use rcc_hir::{Def, DefId, DefKind, HirCrate, Linkage, RecordKind, TyCtxt};
use rcc_session::Session;
use rcc_span::Symbol;

/// Entry point: lower an AST into a fresh `HirCrate`.
///
/// Currently implements only the first-pass DefId assignment (task 06-01).
/// Further lowering (name resolution, type flattening, etc.) will be added
/// in subsequent tasks.
pub fn lower(ast: &TranslationUnit, tcx: &mut TyCtxt, session: &mut Session) -> HirCrate {
    let mut crate_ = HirCrate::default();
    let mut resolver = Resolver::default();
    assign_def_ids(ast, tcx, session, &mut crate_, &mut resolver);
    crate_
}

/// Per-crate resolution tables built while lowering.
#[derive(Default, Debug)]
pub struct Resolver {
    /// Ordinary namespace: (name) -> `DefId`.
    pub ordinary: FxHashMap<Symbol, DefId>,
    /// Tag namespace: `struct`/`union`/`enum` tags.
    pub tags: FxHashMap<Symbol, DefId>,
    /// Labels are strictly per-function; populated then flushed per body.
    pub labels: FxHashMap<Symbol, rcc_hir::HirStmtId>,
}

/// First-pass: walk the AST top-level and assign a `DefId` to every
/// function definition, global variable, typedef, and struct/union/enum tag.
///
/// Populates `crate_.defs`, `resolver.ordinary`, and `resolver.tags`.
/// Conflict detection is deferred to task 02.
fn assign_def_ids(
    ast: &TranslationUnit,
    tcx: &TyCtxt,
    _session: &mut Session,
    crate_: &mut HirCrate,
    resolver: &mut Resolver,
) {
    for ext_decl in &ast.decls {
        match ext_decl {
            ExternalDecl::Function(func_def) => {
                // Function definition — extract name from declarator.
                if let Some((name, _span)) = func_def.declarator.name {
                    let id = crate_.defs.push(Def {
                        id: DefId(0), // patched below
                        name,
                        span: func_def.span,
                        kind: DefKind::Function {
                            ty: tcx.error,
                            has_body: true,
                            is_static: func_def.specs.storage == Some(StorageClass::Static),
                            is_inline: func_def.specs.func_specs.inline,
                            variadic: false,
                        },
                    });
                    crate_.defs[id].id = id;
                    resolver.ordinary.insert(name, id);
                }
            }
            ExternalDecl::Decl(decl) => {
                let is_typedef = decl.specs.storage == Some(StorageClass::Typedef);

                // Scan type specifiers for tag definitions (struct/union/enum).
                for ts in &decl.specs.type_specs {
                    match ts {
                        TypeSpec::Record(rec) => {
                            // Only register when defining (fields present) and tag exists.
                            if let (Some(tag), Some(_fields)) = (rec.tag, &rec.fields) {
                                let kind = match rec.kind {
                                    rcc_ast::RecordKind::Struct => RecordKind::Struct,
                                    rcc_ast::RecordKind::Union => RecordKind::Union,
                                };
                                let id = crate_.defs.push(Def {
                                    id: DefId(0),
                                    name: tag,
                                    span: rec.span,
                                    kind: DefKind::Record {
                                        kind,
                                        layout: None,
                                        fields: Vec::new(),
                                    },
                                });
                                crate_.defs[id].id = id;
                                resolver.tags.insert(tag, id);
                            }
                        }
                        TypeSpec::Enum(en) => {
                            // Only register when defining (enumerators present) and tag exists.
                            if let (Some(tag), Some(_enumerators)) = (en.tag, &en.enumerators) {
                                let id = crate_.defs.push(Def {
                                    id: DefId(0),
                                    name: tag,
                                    span: en.span,
                                    kind: DefKind::Enum { repr: tcx.int, variants: Vec::new() },
                                });
                                crate_.defs[id].id = id;
                                resolver.tags.insert(tag, id);
                            }
                        }
                        _ => {}
                    }
                }

                // Process each init-declarator.
                for init_decl in &decl.inits {
                    if let Some((name, _span)) = init_decl.declarator.name {
                        if is_typedef {
                            let id = crate_.defs.push(Def {
                                id: DefId(0),
                                name,
                                span: decl.span,
                                kind: DefKind::Typedef(tcx.error),
                            });
                            crate_.defs[id].id = id;
                            resolver.ordinary.insert(name, id);
                        } else {
                            // Global variable (or extern declaration).
                            let linkage = match decl.specs.storage {
                                Some(StorageClass::Static) => Linkage::Internal,
                                Some(StorageClass::Extern) => Linkage::External,
                                _ => Linkage::External,
                            };
                            let id = crate_.defs.push(Def {
                                id: DefId(0),
                                name,
                                span: decl.span,
                                kind: DefKind::Global { ty: tcx.error, linkage },
                            });
                            crate_.defs[id].id = id;
                            resolver.ordinary.insert(name, id);
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rcc_ast::{
        Block, Decl, DeclSpecs, Declarator, EnumSpec, ExternalDecl, FunctionDef, InitDeclarator,
        NodeId, RecordSpec, TranslationUnit, TypeSpec,
    };
    use rcc_hir::TyCtxt;
    use rcc_session::Session;
    use rcc_span::DUMMY_SP;

    /// Helper: intern a name symbol via the session interner.
    fn sym(sess: &mut Session, s: &str) -> Symbol {
        sess.interner.intern(s)
    }

    /// Helper: build a minimal declarator with just a name.
    fn named_declarator(name: Symbol) -> Declarator {
        Declarator { name: Some((name, DUMMY_SP)), derived: Vec::new(), span: DUMMY_SP }
    }

    /// Helper: default DeclSpecs (no storage class, empty type specs).
    fn default_specs() -> DeclSpecs {
        DeclSpecs::default()
    }

    /// Helper: a minimal empty compound block (function body).
    fn empty_body() -> Block {
        Block { id: NodeId(0), items: Vec::new(), span: DUMMY_SP }
    }

    /// Helper: make a function definition `ExternalDecl`.
    fn make_func(name: Symbol) -> ExternalDecl {
        ExternalDecl::Function(FunctionDef {
            id: NodeId(0),
            span: DUMMY_SP,
            specs: default_specs(),
            declarator: named_declarator(name),
            kr_decls: Vec::new(),
            body: empty_body(),
        })
    }

    /// Helper: make a global variable declaration `ExternalDecl`.
    fn make_global(name: Symbol) -> ExternalDecl {
        ExternalDecl::Decl(Decl {
            id: NodeId(0),
            span: DUMMY_SP,
            specs: {
                let mut s = default_specs();
                s.type_specs.push(TypeSpec::Int);
                s
            },
            inits: vec![InitDeclarator { declarator: named_declarator(name), init: None }],
        })
    }

    /// Helper: make a typedef declaration `ExternalDecl`.
    fn make_typedef(name: Symbol) -> ExternalDecl {
        ExternalDecl::Decl(Decl {
            id: NodeId(0),
            span: DUMMY_SP,
            specs: {
                let mut s = default_specs();
                s.storage = Some(StorageClass::Typedef);
                s.type_specs.push(TypeSpec::Int);
                s
            },
            inits: vec![InitDeclarator { declarator: named_declarator(name), init: None }],
        })
    }

    /// Helper: make a `struct tag { ... }` declaration (defining, with empty fields).
    fn make_struct(tag: Symbol) -> ExternalDecl {
        ExternalDecl::Decl(Decl {
            id: NodeId(0),
            span: DUMMY_SP,
            specs: {
                let mut s = default_specs();
                s.type_specs.push(TypeSpec::Record(RecordSpec {
                    id: NodeId(0),
                    kind: rcc_ast::RecordKind::Struct,
                    tag: Some(tag),
                    fields: Some(Vec::new()),
                    span: DUMMY_SP,
                }));
                s
            },
            inits: Vec::new(),
        })
    }

    /// Helper: make an `enum tag { ... }` declaration (defining, with empty enumerators).
    fn make_enum(tag: Symbol) -> ExternalDecl {
        ExternalDecl::Decl(Decl {
            id: NodeId(0),
            span: DUMMY_SP,
            specs: {
                let mut s = default_specs();
                s.type_specs.push(TypeSpec::Enum(EnumSpec {
                    id: NodeId(0),
                    tag: Some(tag),
                    enumerators: Some(Vec::new()),
                    span: DUMMY_SP,
                }));
                s
            },
            inits: Vec::new(),
        })
    }

    #[test]
    fn empty_tu_produces_no_defs() {
        let ast = TranslationUnit { decls: Vec::new(), span: DUMMY_SP };
        let mut tcx = TyCtxt::new();
        let (mut sess, _cap) = Session::for_test();
        let hir = lower(&ast, &mut tcx, &mut sess);
        assert_eq!(hir.defs.len(), 0);
    }

    #[test]
    fn single_function_gets_one_def() {
        let (mut sess, _cap) = Session::for_test();
        let name = sym(&mut sess, "main");
        let ast = TranslationUnit { decls: vec![make_func(name)], span: DUMMY_SP };
        let mut tcx = TyCtxt::new();
        let hir = lower(&ast, &mut tcx, &mut sess);
        assert_eq!(hir.defs.len(), 1);
        assert_eq!(hir.defs[DefId(0)].name, name);
        assert!(matches!(hir.defs[DefId(0)].kind, DefKind::Function { .. }));
    }

    #[test]
    fn global_variable_gets_one_def() {
        let (mut sess, _cap) = Session::for_test();
        let name = sym(&mut sess, "counter");
        let ast = TranslationUnit { decls: vec![make_global(name)], span: DUMMY_SP };
        let mut tcx = TyCtxt::new();
        let hir = lower(&ast, &mut tcx, &mut sess);
        assert_eq!(hir.defs.len(), 1);
        assert!(matches!(hir.defs[DefId(0)].kind, DefKind::Global { .. }));
    }

    #[test]
    fn typedef_gets_one_def() {
        let (mut sess, _cap) = Session::for_test();
        let name = sym(&mut sess, "uint32");
        let ast = TranslationUnit { decls: vec![make_typedef(name)], span: DUMMY_SP };
        let mut tcx = TyCtxt::new();
        let hir = lower(&ast, &mut tcx, &mut sess);
        assert_eq!(hir.defs.len(), 1);
        assert!(matches!(hir.defs[DefId(0)].kind, DefKind::Typedef(_)));
    }

    #[test]
    fn struct_tag_gets_one_def() {
        let (mut sess, _cap) = Session::for_test();
        let tag = sym(&mut sess, "point");
        let ast = TranslationUnit { decls: vec![make_struct(tag)], span: DUMMY_SP };
        let mut tcx = TyCtxt::new();
        let hir = lower(&ast, &mut tcx, &mut sess);
        assert_eq!(hir.defs.len(), 1);
        assert!(matches!(
            hir.defs[DefId(0)].kind,
            DefKind::Record { kind: RecordKind::Struct, .. }
        ));
    }

    #[test]
    fn enum_tag_gets_one_def() {
        let (mut sess, _cap) = Session::for_test();
        let tag = sym(&mut sess, "color");
        let ast = TranslationUnit { decls: vec![make_enum(tag)], span: DUMMY_SP };
        let mut tcx = TyCtxt::new();
        let hir = lower(&ast, &mut tcx, &mut sess);
        assert_eq!(hir.defs.len(), 1);
        assert!(matches!(hir.defs[DefId(0)].kind, DefKind::Enum { .. }));
    }

    #[test]
    fn mixed_toplevel_assigns_correct_count() {
        // Simulate:  void f(); int g; typedef int T; struct S {}; enum E {};
        // Expected: 5 DefIds total.
        let (mut sess, _cap) = Session::for_test();
        let f = sym(&mut sess, "f");
        let g = sym(&mut sess, "g");
        let t = sym(&mut sess, "T");
        let s = sym(&mut sess, "S");
        let e = sym(&mut sess, "E");

        let ast = TranslationUnit {
            decls: vec![
                make_func(f),
                make_global(g),
                make_typedef(t),
                make_struct(s),
                make_enum(e),
            ],
            span: DUMMY_SP,
        };
        let mut tcx = TyCtxt::new();
        let hir = lower(&ast, &mut tcx, &mut sess);
        assert_eq!(hir.defs.len(), 5, "expected 5 defs, got {}", hir.defs.len());
    }

    #[test]
    fn def_ids_are_sequential() {
        let (mut sess, _cap) = Session::for_test();
        let a = sym(&mut sess, "a");
        let b = sym(&mut sess, "b");
        let c = sym(&mut sess, "c");

        let ast = TranslationUnit {
            decls: vec![make_func(a), make_global(b), make_typedef(c)],
            span: DUMMY_SP,
        };
        let mut tcx = TyCtxt::new();
        let hir = lower(&ast, &mut tcx, &mut sess);
        assert_eq!(hir.defs.len(), 3);
        for (i, (id, def)) in hir.defs.iter_enumerated().enumerate() {
            assert_eq!(id.0 as usize, i, "DefId should be sequential");
            assert_eq!(def.id, id, "Def.id should match its index");
        }
    }

    #[test]
    fn resolver_ordinary_populated() {
        let (mut sess, _cap) = Session::for_test();
        let f = sym(&mut sess, "f");
        let g = sym(&mut sess, "g");

        let ast = TranslationUnit { decls: vec![make_func(f), make_global(g)], span: DUMMY_SP };
        let tcx = TyCtxt::new();
        let mut crate_ = HirCrate::default();
        let mut resolver = Resolver::default();
        assign_def_ids(&ast, &tcx, &mut sess, &mut crate_, &mut resolver);

        assert_eq!(resolver.ordinary.len(), 2);
        assert!(resolver.ordinary.contains_key(&f));
        assert!(resolver.ordinary.contains_key(&g));
    }

    #[test]
    fn resolver_tags_populated() {
        let (mut sess, _cap) = Session::for_test();
        let s = sym(&mut sess, "S");
        let e = sym(&mut sess, "E");

        let ast = TranslationUnit { decls: vec![make_struct(s), make_enum(e)], span: DUMMY_SP };
        let tcx = TyCtxt::new();
        let mut crate_ = HirCrate::default();
        let mut resolver = Resolver::default();
        assign_def_ids(&ast, &tcx, &mut sess, &mut crate_, &mut resolver);

        assert_eq!(resolver.tags.len(), 2);
        assert!(resolver.tags.contains_key(&s));
        assert!(resolver.tags.contains_key(&e));
        // Tags should NOT appear in ordinary namespace.
        assert!(resolver.ordinary.is_empty());
    }

    #[test]
    fn struct_with_tag_variable_produces_two_defs() {
        // `struct S { int x; } s;` — one tag def + one global variable def.
        let (mut sess, _cap) = Session::for_test();
        let tag = sym(&mut sess, "S");
        let var = sym(&mut sess, "s");

        let ast = TranslationUnit {
            decls: vec![ExternalDecl::Decl(Decl {
                id: NodeId(0),
                span: DUMMY_SP,
                specs: {
                    let mut s = default_specs();
                    s.type_specs.push(TypeSpec::Record(RecordSpec {
                        id: NodeId(0),
                        kind: rcc_ast::RecordKind::Struct,
                        tag: Some(tag),
                        fields: Some(Vec::new()),
                        span: DUMMY_SP,
                    }));
                    s
                },
                inits: vec![InitDeclarator { declarator: named_declarator(var), init: None }],
            })],
            span: DUMMY_SP,
        };
        let mut tcx = TyCtxt::new();
        let hir = lower(&ast, &mut tcx, &mut sess);
        assert_eq!(hir.defs.len(), 2, "tag + variable = 2 defs");
    }

    #[test]
    fn bare_struct_ref_no_def() {
        // `struct S;` (forward declaration, no field body) — no tag def created.
        let (mut sess, _cap) = Session::for_test();
        let tag = sym(&mut sess, "S");

        let ast = TranslationUnit {
            decls: vec![ExternalDecl::Decl(Decl {
                id: NodeId(0),
                span: DUMMY_SP,
                specs: {
                    let mut s = default_specs();
                    s.type_specs.push(TypeSpec::Record(RecordSpec {
                        id: NodeId(0),
                        kind: rcc_ast::RecordKind::Struct,
                        tag: Some(tag),
                        fields: None, // no definition
                        span: DUMMY_SP,
                    }));
                    s
                },
                inits: Vec::new(),
            })],
            span: DUMMY_SP,
        };
        let mut tcx = TyCtxt::new();
        let hir = lower(&ast, &mut tcx, &mut sess);
        assert_eq!(hir.defs.len(), 0, "bare struct ref should not create a def");
    }
}
