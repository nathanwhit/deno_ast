// Copyright 2018-2024 the Deno authors. All rights reserved. MIT license.

use oxc::allocator::Allocator;
use oxc::allocator::ArenaVec;
use oxc::allocator::CloneIn;
use oxc::allocator::GetAllocator;
use oxc::ast::ast::*;
use oxc::ast::builder::AstBuilder;
use oxc::ast_visit::VisitMut;
use oxc::ast_visit::walk_mut;
use oxc::span::SPAN;

use crate::JSStrExt;

/// Transforms import declarations to variable declarations
/// with a dynamic import. This is used to provide import
/// declaration support in script contexts such as the Deno REPL.
pub struct ImportDeclsToVarDecls<'a> {
  ast: AstBuilder<'a>,
}

impl<'a> ImportDeclsToVarDecls<'a> {
  pub fn new(allocator: &'a Allocator) -> Self {
    Self {
      ast: AstBuilder::new(allocator),
    }
  }
}

impl<'a> VisitMut<'a> for ImportDeclsToVarDecls<'a> {
  fn visit_statements(
    &mut self,
    stmts: &mut oxc::allocator::Vec<'a, Statement<'a>>,
  ) {
    let mut new_stmts = ArenaVec::with_capacity_in(stmts.len(), &self.ast);
    for stmt in stmts.drain(..) {
      match stmt {
        Statement::ImportDeclaration(import_decl) => {
          if import_decl.import_kind.is_type() {
            // type-only import - remove entirely
            continue;
          }

          let source_value = import_decl.source.value.to_str_lossy();

          // Side-effect only import: `import "./mod.ts"` -> `await import("./mod.ts")`
          if import_decl.specifiers.as_ref().is_none_or(|s| s.is_empty()) {
            let import_expr = self.create_await_import_expr(&source_value);
            new_stmts.push(Statement::new_expression_statement(
              SPAN,
              import_expr,
              &self.ast,
            ));
            continue;
          }

          let specifiers = import_decl.specifiers.as_ref().unwrap();

          // Collect named/default imports for destructuring
          let mut props = ArenaVec::new_in(&self.ast);
          let mut namespace_name: Option<&str> = None;

          for specifier in specifiers {
            match specifier {
              ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => {
                let key = PropertyKey::new_static_identifier(
                  SPAN, "default", &self.ast,
                );
                let value = BindingPattern::new_binding_identifier(
                  SPAN,
                  s.local.name.as_str(),
                  &self.ast,
                );
                props.push(BindingProperty::new(
                  SPAN, key, value, false, false, &self.ast,
                ));
              }
              ImportDeclarationSpecifier::ImportSpecifier(s) => {
                let imported_name = s.imported.name();
                let local_name = s.local.name.as_str();
                let shorthand = imported_name == local_name;
                let key = PropertyKey::new_static_identifier(
                  SPAN,
                  imported_name,
                  &self.ast,
                );
                let value = BindingPattern::new_binding_identifier(
                  SPAN, local_name, &self.ast,
                );
                props.push(BindingProperty::new(
                  SPAN, key, value, shorthand, false, &self.ast,
                ));
              }
              ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => {
                namespace_name = Some(s.local.name.as_str());
              }
            }
          }

          let import_expr = self.create_await_import_expr(&source_value);

          let mut declarators = ArenaVec::new_in(&self.ast);

          if !props.is_empty() {
            let pattern =
              BindingPattern::new_object_pattern(SPAN, props, None, &self.ast);
            declarators.push(VariableDeclarator::new(
              SPAN,
              pattern,
              None,
              Some(import_expr.clone_in(self.ast.allocator())),
              false,
              &self.ast,
            ));
          }

          if let Some(ns_name) = namespace_name {
            let pattern =
              BindingPattern::new_binding_identifier(SPAN, ns_name, &self.ast);
            declarators.push(VariableDeclarator::new(
              SPAN,
              pattern,
              None,
              Some(import_expr),
              false,
              &self.ast,
            ));
          }

          if !declarators.is_empty() {
            let var_decl =
              Statement::from(Declaration::new_variable_declaration(
                SPAN,
                VariableDeclarationKind::Const,
                declarators,
                false,
                &self.ast,
              ));
            new_stmts.push(var_decl);
          }
        }
        _ => {
          new_stmts.push(stmt);
        }
      }
    }
    *stmts = new_stmts;

    // Visit children
    walk_mut::walk_statements(self, stmts);
  }
}

impl<'a> ImportDeclsToVarDecls<'a> {
  fn create_await_import_expr(&self, source: &str) -> Expression<'a> {
    let source_in_alloc =
      oxc::allocator::StringBuilder::from_str_in(source, self.ast.allocator());
    let import_arg = Expression::new_string_literal(
      SPAN,
      source_in_alloc.into_str(),
      None,
      &self.ast,
    );
    let import_call = Expression::new_import_expression(
      SPAN, import_arg, None, None, &self.ast,
    );
    Expression::new_await_expression(SPAN, import_call, &self.ast)
  }
}

/// Strips export declarations and exports on named exports so the
/// code can be used in script contexts. This is useful for example
/// in the Deno REPL.
pub struct StripExports<'a> {
  ast: AstBuilder<'a>,
}

impl<'a> StripExports<'a> {
  pub fn new(allocator: &'a Allocator) -> Self {
    Self {
      ast: AstBuilder::new(allocator),
    }
  }
}

impl<'a> VisitMut<'a> for StripExports<'a> {
  fn visit_statements(
    &mut self,
    stmts: &mut oxc::allocator::Vec<'a, Statement<'a>>,
  ) {
    let mut new_stmts = ArenaVec::with_capacity_in(stmts.len(), &self.ast);
    for stmt in stmts.drain(..) {
      match stmt {
        Statement::ExportAllDeclaration(export_all) => {
          let source_value = export_all.source.value.to_str_lossy();
          let import_expr = self.create_await_import_expr(&source_value);
          new_stmts.push(Statement::new_expression_statement(
            SPAN,
            import_expr,
            &self.ast,
          ));
        }
        Statement::ExportFromDeclaration(export_from) => {
          let source_value = export_from.source.value.to_str_lossy();
          let import_expr = self.create_await_import_expr(&source_value);
          new_stmts.push(Statement::new_expression_statement(
            SPAN,
            import_expr,
            &self.ast,
          ));
        }
        Statement::ExportDeclaration(export_decl) => {
          new_stmts.push(Statement::from(export_decl.unbox().declaration));
        }
        Statement::ExportNamedDeclaration(_) => {
          // `export { test }` -> remove entirely
        }
        Statement::ExportDefaultDeclaration(export_default) => {
          match export_default.unbox().declaration {
            ExportDefaultDeclarationKind::FunctionDeclaration(func) => {
              if func.id.is_some() {
                new_stmts.push(Statement::FunctionDeclaration(func));
              }
            }
            ExportDefaultDeclarationKind::ClassDeclaration(class) => {
              if class.id.is_some() {
                new_stmts.push(Statement::ClassDeclaration(class));
              }
            }
            ExportDefaultDeclarationKind::TSInterfaceDeclaration(_) => {
              // type only - discard
            }
            kind => {
              // `export default <expr>` -> `<expr>;`
              // ExportDefaultDeclarationKind inherits Expression variants,
              // so all remaining variants are expressions.
              if kind.is_expression() {
                let expr = kind.into_expression();
                new_stmts.push(Statement::new_expression_statement(
                  SPAN, expr, &self.ast,
                ));
              }
            }
          }
        }
        _ => {
          new_stmts.push(stmt);
        }
      }
    }
    *stmts = new_stmts;
    // Don't recurse into nested scopes - only strip top-level exports
  }
}

impl<'a> StripExports<'a> {
  fn create_await_import_expr(&self, source: &str) -> Expression<'a> {
    let source_in_alloc =
      oxc::allocator::StringBuilder::from_str_in(source, self.ast.allocator());
    let import_arg = Expression::new_string_literal(
      SPAN,
      source_in_alloc.into_str(),
      None,
      &self.ast,
    );
    let import_call = Expression::new_import_expression(
      SPAN, import_arg, None, None, &self.ast,
    );
    Expression::new_await_expression(SPAN, import_call, &self.ast)
  }
}

#[cfg(test)]
mod test {
  use oxc::allocator::Allocator;
  use oxc::codegen::Codegen;
  use oxc::parser::Parser;
  use oxc::span::SourceType;
  use pretty_assertions::assert_eq;

  use super::*;

  #[track_caller]
  fn test_transform_import(src: &str, expected_output: &str) {
    let allocator = Allocator::default();
    let mut transform = ImportDeclsToVarDecls::new(&allocator);
    run_transform(&allocator, &mut transform, src, expected_output);
  }

  #[track_caller]
  fn test_transform_strip(src: &str, expected_output: &str) {
    let allocator = Allocator::default();
    let mut transform = StripExports::new(&allocator);
    run_transform(&allocator, &mut transform, src, expected_output);
  }

  #[track_caller]
  fn run_transform<'a>(
    allocator: &'a Allocator,
    transform: &mut impl VisitMut<'a>,
    src: &str,
    expected_output: &str,
  ) {
    let source_type = SourceType::tsx();
    let src_in_arena = allocator.alloc_str(src);
    let mut ret = Parser::new(allocator, src_in_arena, source_type).parse();
    transform.visit_program(&mut ret.program);
    let output = Codegen::new().build(&ret.program).code;
    let output = output.trim_end();
    assert_eq!(output, expected_output);
  }

  // === ImportDeclsToVarDecls tests ===

  #[test]
  fn test_downlevel_imports_type_only() {
    test_transform_import(r#"import type { test } from "./mod.ts";"#, "");
  }

  #[test]
  fn test_downlevel_imports_specifier_only() {
    test_transform_import(
      r#"import "./mod.ts";"#,
      r#"await import("./mod.ts");"#,
    );
    test_transform_import(
      r#"import {} from "./mod.ts";"#,
      r#"await import("./mod.ts");"#,
    );
  }

  #[test]
  fn test_downlevel_imports_default() {
    test_transform_import(
      r#"import mod from "./mod.ts";"#,
      r#"const { default: mod } = await import("./mod.ts");"#,
    );
  }

  #[test]
  fn test_downlevel_imports_named() {
    test_transform_import(
      r#"import { A } from "./mod.ts";"#,
      r#"const { A } = await import("./mod.ts");"#,
    );
    test_transform_import(
      r#"import { A, B, C } from "./mod.ts";"#,
      r#"const { A, B, C } = await import("./mod.ts");"#,
    );
    test_transform_import(
      r#"import { A as LocalA, B, C as LocalC } from "./mod.ts";"#,
      r#"const { A: LocalA, B, C: LocalC } = await import("./mod.ts");"#,
    );
  }

  #[test]
  fn test_downlevel_imports_namespace() {
    test_transform_import(
      r#"import * as mod from "./mod.ts";"#,
      r#"const mod = await import("./mod.ts");"#,
    );
  }

  #[test]
  fn test_downlevel_imports_mixed() {
    test_transform_import(
      r#"import myDefault, { A, B as LocalB } from "./mod.ts";"#,
      r#"const { default: myDefault, A, B: LocalB } = await import("./mod.ts");"#,
    );
    test_transform_import(
      r#"import myDefault, * as mod from "./mod.ts";"#,
      r#"const { default: myDefault } = await import("./mod.ts"), mod = await import("./mod.ts");"#,
    );
  }

  #[test]
  fn test_downlevel_imports_assertions() {
    test_transform_import(
      r#"import data from "./mod.json" with { type: "json" };"#,
      r#"const { default: data } = await import("./mod.json");"#,
    );
  }

  // === StripExports tests ===

  #[test]
  fn test_strip_exports_export_all() {
    test_transform_strip(
      r#"export * from "./test.ts";"#,
      r#"await import("./test.ts");"#,
    );
  }

  #[test]
  fn test_strip_exports_export_named() {
    test_transform_strip(
      r#"export { test } from "./test.ts";"#,
      r#"await import("./test.ts");"#,
    );
    test_transform_strip(r#"export { test };"#, "");
  }

  #[test]
  fn test_strip_exports_assertions() {
    test_transform_strip(
      r#"export { default as data } from "./mod.json" with { type: "json" };"#,
      r#"await import("./mod.json");"#,
    );
  }

  #[test]
  fn test_strip_exports_export_all_assertions() {
    test_transform_strip(
      r#"export * from "./mod.json" with { type: "json" };"#,
      r#"await import("./mod.json");"#,
    );
  }

  #[test]
  fn test_strip_exports_export_default_expr() {
    test_transform_strip("export default 5;", "5;");
  }

  #[test]
  fn test_strip_exports_export_default_decl_name() {
    test_transform_strip("export default class Test {}", "class Test {}");
    test_transform_strip(
      "export default function test() {}",
      "function test() {}",
    );
  }

  #[test]
  fn test_strip_exports_export_default_decl_no_name() {
    test_transform_strip("export default class {}", "");
    test_transform_strip("export default function() {}", "");
  }

  #[test]
  fn test_strip_exports_export_named_decls() {
    test_transform_strip("export class Test {}", "class Test {}");
    test_transform_strip("export function test() {}", "function test() {}");
    test_transform_strip("export enum Test {}", "enum Test {}");
    test_transform_strip("export namespace Test {}", "namespace Test {}");
  }

  #[test]
  fn test_strip_exports_not_in_namespace() {
    test_transform_strip(
      "namespace Test { export class Test {} }",
      "namespace Test {\n\texport class Test {}\n}",
    );
  }
}
