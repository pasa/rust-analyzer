use itertools::Itertools;
use hir::{Problem, source_binder};
use ra_ide_api_light::Severity;
use ra_db::SourceDatabase;
use ra_syntax::{
    Location, SourceFile, SyntaxKind, TextRange, SyntaxNode,
    ast::{self, AstNode, NameOwner},

};
use ra_text_edit::{TextEdit, TextEditBuilder};

use crate::{Diagnostic, FileId, FileSystemEdit, SourceChange, SourceFileEdit, db::RootDatabase};

pub(crate) fn diagnostics(db: &RootDatabase, file_id: FileId) -> Vec<Diagnostic> {
    let source_file = db.parse(file_id);
    let mut res = Vec::new();

    syntax_errors(&mut res, &source_file);

    for node in source_file.syntax().descendants() {
        check_unnecessary_braces_in_use_statement(&mut res, file_id, node);
        check_struct_shorthand_initialization(&mut res, file_id, node);
    }

    if let Some(m) = source_binder::module_from_file_id(db, file_id) {
        check_module(&mut res, db, file_id, m);
    };
    res
}

fn syntax_errors(acc: &mut Vec<Diagnostic>, source_file: &SourceFile) {
    fn location_to_range(location: Location) -> TextRange {
        match location {
            Location::Offset(offset) => TextRange::offset_len(offset, 1.into()),
            Location::Range(range) => range,
        }
    }

    acc.extend(source_file.errors().into_iter().map(|err| Diagnostic {
        range: location_to_range(err.location()),
        message: format!("Syntax Error: {}", err),
        severity: Severity::Error,
        fix: None,
    }));
}

fn check_unnecessary_braces_in_use_statement(
    acc: &mut Vec<Diagnostic>,
    file_id: FileId,
    node: &SyntaxNode,
) -> Option<()> {
    let use_tree_list = ast::UseTreeList::cast(node)?;
    if let Some((single_use_tree,)) = use_tree_list.use_trees().collect_tuple() {
        let range = use_tree_list.syntax().range();
        let edit =
            text_edit_for_remove_unnecessary_braces_with_self_in_use_statement(single_use_tree)
                .unwrap_or_else(|| {
                    let to_replace = single_use_tree.syntax().text().to_string();
                    let mut edit_builder = TextEditBuilder::default();
                    edit_builder.delete(range);
                    edit_builder.insert(range.start(), to_replace);
                    edit_builder.finish()
                });

        acc.push(Diagnostic {
            range,
            message: format!("Unnecessary braces in use statement"),
            severity: Severity::WeakWarning,
            fix: Some(SourceChange {
                label: "Remove unnecessary braces".to_string(),
                source_file_edits: vec![SourceFileEdit { file_id, edit }],
                file_system_edits: Vec::new(),
                cursor_position: None,
            }),
        });
    }

    Some(())
}

fn text_edit_for_remove_unnecessary_braces_with_self_in_use_statement(
    single_use_tree: &ast::UseTree,
) -> Option<TextEdit> {
    let use_tree_list_node = single_use_tree.syntax().parent()?;
    if single_use_tree.path()?.segment()?.syntax().first_child()?.kind() == SyntaxKind::SELF_KW {
        let start = use_tree_list_node.prev_sibling()?.range().start();
        let end = use_tree_list_node.range().end();
        let range = TextRange::from_to(start, end);
        let mut edit_builder = TextEditBuilder::default();
        edit_builder.delete(range);
        return Some(edit_builder.finish());
    }
    None
}

fn check_struct_shorthand_initialization(
    acc: &mut Vec<Diagnostic>,
    file_id: FileId,
    node: &SyntaxNode,
) -> Option<()> {
    let struct_lit = ast::StructLit::cast(node)?;
    let named_field_list = struct_lit.named_field_list()?;
    for named_field in named_field_list.fields() {
        if let (Some(name_ref), Some(expr)) = (named_field.name_ref(), named_field.expr()) {
            let field_name = name_ref.syntax().text().to_string();
            let field_expr = expr.syntax().text().to_string();
            if field_name == field_expr {
                let mut edit_builder = TextEditBuilder::default();
                edit_builder.delete(named_field.syntax().range());
                edit_builder.insert(named_field.syntax().range().start(), field_name);
                let edit = edit_builder.finish();

                acc.push(Diagnostic {
                    range: named_field.syntax().range(),
                    message: format!("Shorthand struct initialization"),
                    severity: Severity::WeakWarning,
                    fix: Some(SourceChange {
                        label: "use struct shorthand initialization".to_string(),
                        source_file_edits: vec![SourceFileEdit { file_id, edit }],
                        file_system_edits: Vec::new(),
                        cursor_position: None,
                    }),
                });
            }
        }
    }
    Some(())
}

fn check_module(
    acc: &mut Vec<Diagnostic>,
    db: &RootDatabase,
    file_id: FileId,
    module: hir::Module,
) {
    for decl in module.declarations(db) {
        match decl {
            hir::ModuleDef::Function(f) => check_function(acc, db, f),
            _ => (),
        }
    }

    let source_root = db.file_source_root(file_id);
    for (name_node, problem) in module.problems(db) {
        let diag = match problem {
            Problem::UnresolvedModule { candidate } => {
                let create_file =
                    FileSystemEdit::CreateFile { source_root, path: candidate.clone() };
                let fix = SourceChange {
                    label: "create module".to_string(),
                    source_file_edits: Vec::new(),
                    file_system_edits: vec![create_file],
                    cursor_position: None,
                };
                Diagnostic {
                    range: name_node.range(),
                    message: "unresolved module".to_string(),
                    severity: Severity::Error,
                    fix: Some(fix),
                }
            }
            Problem::NotDirOwner { move_to, candidate } => {
                let move_file = FileSystemEdit::MoveFile {
                    src: file_id,
                    dst_source_root: source_root,
                    dst_path: move_to.clone(),
                };
                let create_file =
                    FileSystemEdit::CreateFile { source_root, path: move_to.join(candidate) };
                let fix = SourceChange {
                    label: "move file and create module".to_string(),
                    source_file_edits: Vec::new(),
                    file_system_edits: vec![move_file, create_file],
                    cursor_position: None,
                };
                Diagnostic {
                    range: name_node.range(),
                    message: "can't declare module at this location".to_string(),
                    severity: Severity::Error,
                    fix: Some(fix),
                }
            }
        };
        acc.push(diag)
    }
}

fn check_function(acc: &mut Vec<Diagnostic>, db: &RootDatabase, function: hir::Function) {
    let (_file_id, fn_def) = function.source(db);
    let source_file = fn_def.syntax().ancestors().find_map(ast::SourceFile::cast).unwrap();
    let source_map = function.body_source_map(db);
    for d in function.diagnostics(db) {
        match d {
            hir::diagnostics::FunctionDiagnostic::NoSuchField { expr, field } => {
                if let Some(field) = source_map.field_syntax(expr, field) {
                    let field = field.to_node(&source_file);
                    acc.push(Diagnostic {
                        message: "no such field".into(),
                        range: field.syntax().range(),
                        severity: Severity::Error,
                        fix: None,
                    })
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use test_utils::assert_eq_text;

    use super::*;

    type DiagnosticChecker = fn(&mut Vec<Diagnostic>, FileId, &SyntaxNode) -> Option<()>;

    fn check_not_applicable(code: &str, func: DiagnosticChecker) {
        let file = SourceFile::parse(code);
        let mut diagnostics = Vec::new();
        for node in file.syntax().descendants() {
            func(&mut diagnostics, FileId(0), node);
        }
        assert!(diagnostics.is_empty());
    }

    fn check_apply(before: &str, after: &str, func: DiagnosticChecker) {
        let file = SourceFile::parse(before);
        let mut diagnostics = Vec::new();
        for node in file.syntax().descendants() {
            func(&mut diagnostics, FileId(0), node);
        }
        let diagnostic =
            diagnostics.pop().unwrap_or_else(|| panic!("no diagnostics for:\n{}\n", before));
        let mut fix = diagnostic.fix.unwrap();
        let edit = fix.source_file_edits.pop().unwrap().edit;
        let actual = edit.apply(&before);
        assert_eq_text!(after, &actual);
    }

    #[test]
    fn test_check_unnecessary_braces_in_use_statement() {
        check_not_applicable(
            "
            use a;
            use a::{c, d::e};
        ",
            check_unnecessary_braces_in_use_statement,
        );
        check_apply("use {b};", "use b;", check_unnecessary_braces_in_use_statement);
        check_apply("use a::{c};", "use a::c;", check_unnecessary_braces_in_use_statement);
        check_apply("use a::{self};", "use a;", check_unnecessary_braces_in_use_statement);
        check_apply(
            "use a::{c, d::{e}};",
            "use a::{c, d::e};",
            check_unnecessary_braces_in_use_statement,
        );
    }

    #[test]
    fn test_check_struct_shorthand_initialization() {
        check_not_applicable(
            r#"
            struct A {
                a: &'static str
            }

            fn main() {
                A {
                    a: "hello"
                }
            }
        "#,
            check_struct_shorthand_initialization,
        );

        check_apply(
            r#"
struct A {
    a: &'static str
}

fn main() {
    let a = "haha";
    A {
        a: a
    }
}
        "#,
            r#"
struct A {
    a: &'static str
}

fn main() {
    let a = "haha";
    A {
        a
    }
}
        "#,
            check_struct_shorthand_initialization,
        );

        check_apply(
            r#"
struct A {
    a: &'static str,
    b: &'static str
}

fn main() {
    let a = "haha";
    let b = "bb";
    A {
        a: a,
        b
    }
}
        "#,
            r#"
struct A {
    a: &'static str,
    b: &'static str
}

fn main() {
    let a = "haha";
    let b = "bb";
    A {
        a,
        b
    }
}
        "#,
            check_struct_shorthand_initialization,
        );
    }
}
