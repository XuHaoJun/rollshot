// spike-js-frontend: parser candidate comparison
// Steps 3 (parse+span), 4 (subset validation walker), 5 (Workflow IR extraction)

use std::path::Path;

fn load_fixture(name: &str) -> String {
    let path = Path::new("fixtures").join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("fixture not found: {}", path.display()))
}

// ─── OXC ────────────────────────────────────────────────────────────────────
#[cfg(feature = "oxc")]
mod oxc_candidate {
    use oxc_allocator::Allocator;
    use oxc_ast::ast::*;
    use oxc_parser::{ParseOptions, Parser};
    use oxc_span::SourceType;

    #[derive(Debug)]
    struct Violation {
        kind: &'static str,
        start: u32,
        end: u32,
    }

    struct Walker<'a> {
        src: &'a str,
        violations: Vec<Violation>,
        ir_calls: Vec<String>,
    }

    impl<'a> Walker<'a> {
        fn new(src: &'a str) -> Self {
            Walker { src, violations: Vec::new(), ir_calls: Vec::new() }
        }

        fn byte_to_lc(&self, byte: u32) -> (usize, usize) {
            let s = &self.src[..byte as usize];
            let line = s.bytes().filter(|&b| b == b'\n').count() + 1;
            let col = s.rfind('\n').map(|i| byte as usize - i - 1).unwrap_or(byte as usize);
            (line, col)
        }

        fn reject(&mut self, kind: &'static str, start: u32, end: u32) {
            self.violations.push(Violation { kind, start, end });
        }

        fn walk_stmt(&mut self, stmt: &Statement) {
            match stmt {
                Statement::VariableDeclaration(decl) => {
                    if decl.kind == VariableDeclarationKind::Var {
                        self.reject("var_declaration", decl.span.start, decl.span.end);
                    }
                    for d in &decl.declarations {
                        if let Some(init) = &d.init {
                            self.walk_expr(init);
                        }
                    }
                }
                Statement::WhileStatement(w) => {
                    self.reject("while_loop", w.span.start, w.span.end);
                }
                Statement::FunctionDeclaration(f) => {
                    if f.generator {
                        self.reject("generator_function", f.span.start, f.span.end);
                    } else {
                        self.reject("function_declaration", f.span.start, f.span.end);
                    }
                }
                Statement::ClassDeclaration(c) => {
                    self.reject("class_declaration", c.span.start, c.span.end);
                }
                Statement::ReturnStatement(r) => {
                    if let Some(arg) = &r.argument {
                        self.walk_expr(arg);
                    }
                }
                Statement::ExpressionStatement(e) => {
                    self.walk_expr(&e.expression);
                }
                _ => {}
            }
        }

        fn walk_expr(&mut self, expr: &Expression) {
            match expr {
                Expression::ComputedMemberExpression(e) => {
                    self.reject("dynamic_member_access", e.span.start, e.span.end);
                    self.walk_expr(&e.object);
                }
                Expression::StaticMemberExpression(member) => {
                    self.walk_expr(&member.object);
                }
                Expression::CallExpression(call) => {
                    match &call.callee {
                        Expression::StaticMemberExpression(member) => {
                            // Reflect / Proxy check
                            if let Expression::Identifier(obj) = &member.object {
                                if obj.name == "Reflect" || obj.name == "Proxy" {
                                    self.reject("reflection", call.span.start, call.span.end);
                                }
                                // rollshot.* IR
                                if obj.name == "rollshot" {
                                    self.ir_calls.push(format!("rollshot.{}", member.property.name));
                                }
                            }
                            // chain IR
                            let name = member.property.name.as_str();
                            if matches!(name, "filter" | "map" | "reduce" | "flatMap") {
                                self.ir_calls.push(format!(".{name}"));
                            }
                            self.walk_expr(&member.object);
                        }
                        Expression::ComputedMemberExpression(e) => {
                            // rollshot[k](...) — dynamic callee
                            self.reject("dynamic_member_access", e.span.start, e.span.end);
                            self.walk_expr(&e.object);
                        }
                        other => {
                            self.walk_expr(other);
                        }
                    }
                    for arg in &call.arguments {
                        if let Some(e) = arg.as_expression() {
                            self.walk_expr(e);
                        }
                    }
                }
                Expression::ArrowFunctionExpression(arrow) => {
                    // Detect escaping closures: body contains outer_ident.push(...)
                    if let FunctionBody { statements, .. } = arrow.body.as_ref() {
                        self.check_escaping_closure_in_stmts(statements);
                    }
                    // Also walk arrow body expressions
                    for stmt in &arrow.body.statements {
                        self.walk_stmt(stmt);
                    }
                }
                Expression::ObjectExpression(obj) => {
                    for prop in &obj.properties {
                        if let oxc_ast::ast::ObjectPropertyKind::ObjectProperty(p) = prop {
                            self.walk_expr(&p.value);
                        }
                    }
                }
                Expression::ArrayExpression(arr) => {
                    for el in &arr.elements {
                        if let Some(e) = el.as_expression() {
                            self.walk_expr(e);
                        }
                    }
                }
                Expression::SequenceExpression(seq) => {
                    for e in &seq.expressions {
                        self.walk_expr(e);
                    }
                }
                _ => {}
            }
        }

        fn check_escaping_closure_in_stmts(&mut self, stmts: &[Statement]) {
            for s in stmts {
                if let Statement::ExpressionStatement(e) = s {
                    if let Expression::CallExpression(call) = &e.expression {
                        if let Expression::StaticMemberExpression(member) = &call.callee {
                            // outer_var.push(...) pattern — escaping closure
                            if member.property.name == "push" {
                                if let Expression::Identifier(_) = &member.object {
                                    self.reject(
                                        "escaping_closure",
                                        call.span.start,
                                        call.span.end,
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    pub fn run() {
        println!("\n=== OXC CANDIDATE ===");
        let fixtures: &[(&str, bool)] = &[
            ("valid_detector.js", true),
            ("reject_var.js", false),
            ("reject_while.js", false),
            ("reject_dynamic_access.js", false),
            ("reject_reflect.js", false),
            ("reject_recursion.js", false),
            ("reject_class.js", false),
            ("reject_escaping_closure.js", false),
            ("reject_generator.js", false),
        ];

        for (name, expect_valid) in fixtures {
            let src = super::load_fixture(name);
            let allocator = Allocator::default();
            // Wrap body statements in a function for valid script parsing
            let wrapped = format!("function __spike__() {{ {} }}", src);
            let ret = Parser::new(
                &allocator,
                &wrapped,
                SourceType::default().with_script(true),
            )
            .with_options(ParseOptions::default())
            .parse();

            if !ret.diagnostics.is_empty() {
                println!(
                    "  {name}: PARSE_ERROR — {:?}",
                    ret.diagnostics[0]
                );
                continue;
            }

            let program = ret.program;
            // Extract the function body statements
            let body_stmts = if let Some(Statement::FunctionDeclaration(f)) =
                program.body.first()
            {
                f.body.as_ref().map(|b| b.statements.as_slice()).unwrap_or(&[])
            } else {
                &[]
            };

            let mut walker = Walker::new(&wrapped);
            for stmt in body_stmts {
                walker.walk_stmt(stmt);
            }

            let valid = walker.violations.is_empty();
            let status = if valid == *expect_valid { "OK" } else { "MISMATCH" };

            if valid {
                println!("  {name}: ACCEPT [{status}]");
                // IR extraction for valid fixture
                if !walker.ir_calls.is_empty() {
                    println!("    IR sequence: {:?}", walker.ir_calls);
                }
            } else {
                for v in &walker.violations {
                    let (line, col) = walker.byte_to_lc(v.start);
                    println!(
                        "  {name}: REJECT [{status}] — {} at byte {}..{} (line {}, col {})",
                        v.kind, v.start, v.end, line, col
                    );
                }
            }
        }
    }
}

// ─── SWC ────────────────────────────────────────────────────────────────────
#[cfg(feature = "swc")]
mod swc_candidate {
    use swc_common::{sync::Lrc, FileName, SourceMap};
    use swc_ecma_ast::*;
    use swc_ecma_parser::{lexer::Lexer, Parser, StringInput, Syntax};

    struct Violation {
        kind: &'static str,
        lo: u32,
        hi: u32,
    }

    struct Walker {
        src: String,
        violations: Vec<Violation>,
        ir_calls: Vec<String>,
    }

    impl Walker {
        fn new(src: String) -> Self {
            Walker { src, violations: Vec::new(), ir_calls: Vec::new() }
        }

        fn byte_to_lc(&self, byte: u32) -> (usize, usize) {
            // SWC BytePos starts at 1, so byte is already 0-based after subtracting 1
            let byte = byte as usize;
            let capped = byte.min(self.src.len());
            let s = &self.src[..capped];
            let line = s.bytes().filter(|&b| b == b'\n').count() + 1;
            let col = s.rfind('\n').map(|i| capped - i - 1).unwrap_or(capped);
            (line, col)
        }

        fn reject(&mut self, kind: &'static str, lo: u32, hi: u32) {
            // Normalize SWC BytePos (starts at 1) to 0-based byte offset
            let lo = lo.saturating_sub(1);
            let hi = hi.saturating_sub(1);
            self.violations.push(Violation { kind, lo, hi });
        }

        fn walk_stmt(&mut self, stmt: &Stmt) {
            match stmt {
                Stmt::Decl(Decl::Var(v)) => {
                    if v.kind == VarDeclKind::Var {
                        let lo = v.span.lo.0;
                        let hi = v.span.hi.0;
                        self.reject("var_declaration", lo, hi);
                    }
                    for decl in &v.decls {
                        if let Some(init) = &decl.init {
                            self.walk_expr(init);
                        }
                    }
                }
                Stmt::While(w) => {
                    let lo = w.span.lo.0;
                    let hi = w.span.hi.0;
                    self.reject("while_loop", lo, hi);
                }
                Stmt::Decl(Decl::Fn(f)) => {
                    if f.function.is_generator {
                        let lo = f.function.span.lo.0;
                        let hi = f.function.span.hi.0;
                        self.reject("generator_function", lo, hi);
                    } else {
                        let lo = f.function.span.lo.0;
                        let hi = f.function.span.hi.0;
                        self.reject("function_declaration", lo, hi);
                    }
                }
                Stmt::Decl(Decl::Class(c)) => {
                    let lo = c.class.span.lo.0;
                    let hi = c.class.span.hi.0;
                    self.reject("class_declaration", lo, hi);
                }
                Stmt::Return(r) => {
                    if let Some(arg) = &r.arg {
                        self.walk_expr(arg);
                    }
                }
                Stmt::Expr(e) => {
                    self.walk_expr(&e.expr);
                }
                _ => {}
            }
        }

        fn walk_expr(&mut self, expr: &Expr) {
            match expr {
                Expr::Member(m) => {
                    if matches!(m.prop, MemberProp::Computed(_)) {
                        let lo = m.span.lo.0;
                        let hi = m.span.hi.0;
                        self.reject("dynamic_member_access", lo, hi);
                    }
                    self.walk_expr(&m.obj);
                }
                Expr::Call(call) => {
                    if let Callee::Expr(callee) = &call.callee {
                        if let Expr::Member(m) = callee.as_ref() {
                            // Dynamic/computed member access as callee
                            if matches!(m.prop, MemberProp::Computed(_)) {
                                let lo = m.span.lo.0;
                                let hi = m.span.hi.0;
                                self.reject("dynamic_member_access", lo, hi);
                            }
                            // Reflect/Proxy and rollshot.* checks
                            if let Expr::Ident(obj) = m.obj.as_ref() {
                                if obj.sym == "Reflect" || obj.sym == "Proxy" {
                                    let lo = call.span.lo.0;
                                    let hi = call.span.hi.0;
                                    self.reject("reflection", lo, hi);
                                }
                                if obj.sym == "rollshot" {
                                    if let MemberProp::Ident(prop) = &m.prop {
                                        self.ir_calls.push(format!("rollshot.{}", prop.sym));
                                    }
                                }
                            }
                            // chain calls for IR
                            if let MemberProp::Ident(prop) = &m.prop {
                                if matches!(prop.sym.as_ref(), "filter" | "map" | "reduce" | "flatMap") {
                                    self.ir_calls.push(format!(".{}", prop.sym));
                                }
                            }
                            self.walk_expr(&m.obj);
                        } else {
                            self.walk_expr(callee);
                        }
                    }
                    for arg in &call.args {
                        self.walk_expr(&arg.expr);
                    }
                }
                Expr::Arrow(arrow) => {
                    match &*arrow.body {
                        BlockStmtOrExpr::BlockStmt(block) => {
                            self.check_escaping_closure_in_stmts(&block.stmts);
                        }
                        BlockStmtOrExpr::Expr(e) => {
                            // Expression-body arrow: check for escaping push
                            if let Expr::Call(call) = e.as_ref() {
                                if let Callee::Expr(callee) = &call.callee {
                                    if let Expr::Member(m) = callee.as_ref() {
                                        if let MemberProp::Ident(prop) = &m.prop {
                                            if prop.sym == "push" {
                                                if let Expr::Ident(_) = m.obj.as_ref() {
                                                    let lo = call.span.lo.0;
                                                    let hi = call.span.hi.0;
                                                    self.reject("escaping_closure", lo, hi);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            self.walk_expr(e);
                        }
                    }
                }
                Expr::Object(obj) => {
                    for prop in &obj.props {
                        if let swc_ecma_ast::PropOrSpread::Prop(p) = prop {
                            match p.as_ref() {
                                swc_ecma_ast::Prop::KeyValue(kv) => self.walk_expr(&kv.value),
                                swc_ecma_ast::Prop::Shorthand(_) => {}
                                _ => {}
                            }
                        }
                    }
                }
                Expr::Array(arr) => {
                    for el in arr.elems.iter().flatten() {
                        self.walk_expr(&el.expr);
                    }
                }
                Expr::Seq(seq) => {
                    for e in &seq.exprs {
                        self.walk_expr(e);
                    }
                }
                _ => {}
            }
        }

        fn check_escaping_closure_in_stmts(&mut self, stmts: &[Stmt]) {
            for s in stmts {
                if let Stmt::Expr(e) = s {
                    if let Expr::Call(call) = e.expr.as_ref() {
                        if let Callee::Expr(callee) = &call.callee {
                            if let Expr::Member(m) = callee.as_ref() {
                                if let MemberProp::Ident(prop) = &m.prop {
                                    if prop.sym == "push" {
                                        if let Expr::Ident(_) = m.obj.as_ref() {
                                            let lo = call.span.lo.0;
                                            let hi = call.span.hi.0;
                                            self.reject("escaping_closure", lo, hi);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    pub fn run() {
        println!("\n=== SWC CANDIDATE ===");
        let fixtures: &[(&str, bool)] = &[
            ("valid_detector.js", true),
            ("reject_var.js", false),
            ("reject_while.js", false),
            ("reject_dynamic_access.js", false),
            ("reject_reflect.js", false),
            ("reject_recursion.js", false),
            ("reject_class.js", false),
            ("reject_escaping_closure.js", false),
            ("reject_generator.js", false),
        ];

        let cm: Lrc<SourceMap> = Default::default();

        for (name, expect_valid) in fixtures {
            let src = super::load_fixture(name);
            let wrapped = format!("function __spike__() {{ {} }}", src);
            let fm = cm.new_source_file(
                FileName::Custom(name.to_string()).into(),
                wrapped.clone(),
            );
            let lexer = Lexer::new(
                Syntax::Es(Default::default()),
                Default::default(),
                StringInput::from(&*fm),
                None,
            );
            let mut parser = Parser::new_from(lexer);
            let module = match parser.parse_script() {
                Ok(m) => m,
                Err(e) => {
                    println!("  {name}: PARSE_ERROR — {e:?}");
                    continue;
                }
            };

            let body_stmts = if let Some(Stmt::Decl(Decl::Fn(f))) = module.body.first() {
                if let Some(body) = &f.function.body {
                    body.stmts.as_slice()
                } else {
                    &[]
                }
            } else {
                &[]
            };

            let mut walker = Walker::new(wrapped.clone());
            for stmt in body_stmts {
                walker.walk_stmt(stmt);
            }

            let valid = walker.violations.is_empty();
            let status = if valid == *expect_valid { "OK" } else { "MISMATCH" };

            if valid {
                println!("  {name}: ACCEPT [{status}]");
                if !walker.ir_calls.is_empty() {
                    println!("    IR sequence: {:?}", walker.ir_calls);
                }
            } else {
                for v in &walker.violations {
                    let (line, col) = walker.byte_to_lc(v.lo.saturating_sub(1));
                    println!(
                        "  {name}: REJECT [{status}] — {} at byte {}..{} (line {}, col {})",
                        v.kind,
                        v.lo,
                        v.hi,
                        line,
                        col
                    );
                }
            }
        }
    }
}

// ─── TREE-SITTER ─────────────────────────────────────────────────────────────
#[cfg(feature = "treesitter")]
mod ts_candidate {
    use tree_sitter::{Language, Node, Parser};

    fn node_to_lc(src: &[u8], node: &Node) -> (usize, usize, usize, usize) {
        let start = node.start_position();
        let end = node.end_position();
        (start.row + 1, start.column, end.row + 1, end.column)
    }

    struct Walker<'a> {
        src: &'a [u8],
        violations: Vec<(&'static str, usize, usize, usize, usize)>,
        ir_calls: Vec<String>,
    }

    impl<'a> Walker<'a> {
        fn new(src: &'a [u8]) -> Self {
            Walker { src, violations: Vec::new(), ir_calls: Vec::new() }
        }

        fn reject(&mut self, kind: &'static str, node: &Node) {
            let (sl, sc, el, ec) = node_to_lc(self.src, node);
            self.violations.push((kind, sl, sc, el, ec));
        }

        fn check_escaping_push_in_block(&mut self, block: &Node) {
            // Walk all expression statements in a block looking for ident.push(...)
            let mut cursor = block.walk();
            for stmt in block.named_children(&mut cursor) {
                if stmt.kind() == "expression_statement" {
                    if let Some(expr) = stmt.named_child(0) {
                        if expr.kind() == "call_expression" {
                            if let Some(callee) = expr.child_by_field_name("function") {
                                if callee.kind() == "member_expression" {
                                    if let (Some(obj), Some(prop)) = (
                                        callee.child_by_field_name("object"),
                                        callee.child_by_field_name("property"),
                                    ) {
                                        let prop_text = prop.utf8_text(self.src).unwrap_or("");
                                        // outer identifier (not 'this', not chained member)
                                        if prop_text == "push" && obj.kind() == "identifier" {
                                            self.reject("escaping_closure", &expr);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                // Also recurse into nested blocks
                if stmt.kind() == "statement_block" {
                    self.check_escaping_push_in_block(&stmt);
                }
            }
        }

        fn walk(&mut self, node: Node) {
            match node.kind() {
                "variable_declaration" => {
                    // var keyword check
                    let text = node.utf8_text(self.src).unwrap_or("");
                    if text.starts_with("var ") {
                        self.reject("var_declaration", &node);
                    }
                }
                "while_statement" => {
                    self.reject("while_loop", &node);
                }
                "function_declaration" => {
                    self.reject("function_declaration", &node);
                }
                "generator_function_declaration" => {
                    self.reject("generator_function", &node);
                }
                "class_declaration" => {
                    self.reject("class_declaration", &node);
                }
                "subscript_expression" => {
                    // dynamic/computed member access
                    self.reject("dynamic_member_access", &node);
                }
                "call_expression" => {
                    // Check callee for Reflect.*, rollshot.*, chain methods
                    if let Some(callee) = node.child_by_field_name("function") {
                        if callee.kind() == "member_expression" {
                            if let (Some(obj), Some(prop)) = (
                                callee.child_by_field_name("object"),
                                callee.child_by_field_name("property"),
                            ) {
                                let obj_text = obj.utf8_text(self.src).unwrap_or("");
                                let prop_text = prop.utf8_text(self.src).unwrap_or("");
                                if obj_text == "Reflect" || obj_text == "Proxy" {
                                    self.reject("reflection", &node);
                                }
                                if obj_text == "rollshot" {
                                    self.ir_calls.push(format!("rollshot.{prop_text}"));
                                }
                                if matches!(prop_text, "filter" | "map" | "reduce" | "flatMap") {
                                    self.ir_calls.push(format!(".{prop_text}"));
                                }
                            }
                        } else if callee.kind() == "subscript_expression" {
                            // Dynamic access used as callee: rollshot[k](...)
                            self.reject("dynamic_member_access", &callee);
                        }
                    }
                    // Fall through to normal named_child recursion below
                }
                "arrow_function" => {
                    // Check body for escaping closures (outer_ident.push pattern)
                    if let Some(body) = node.child_by_field_name("body") {
                        if body.kind() == "statement_block" {
                            self.check_escaping_push_in_block(&body);
                        } else if body.kind() == "call_expression" {
                            // Expression-body arrow: (x) => sink.push(x)
                            if let Some(callee) = body.child_by_field_name("function") {
                                if callee.kind() == "member_expression" {
                                    if let (Some(obj), Some(prop)) = (
                                        callee.child_by_field_name("object"),
                                        callee.child_by_field_name("property"),
                                    ) {
                                        let prop_text = prop.utf8_text(self.src).unwrap_or("");
                                        if prop_text == "push" && obj.kind() == "identifier" {
                                            self.reject("escaping_closure", &body);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
            // Recurse
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                self.walk(child);
            }
        }
    }

    pub fn run() {
        println!("\n=== TREE-SITTER CANDIDATE ===");
        let fixtures: &[(&str, bool)] = &[
            ("valid_detector.js", true),
            ("reject_var.js", false),
            ("reject_while.js", false),
            ("reject_dynamic_access.js", false),
            ("reject_reflect.js", false),
            ("reject_recursion.js", false),
            ("reject_class.js", false),
            ("reject_escaping_closure.js", false),
            ("reject_generator.js", false),
        ];

        let mut parser = Parser::new();
        let language: Language = tree_sitter_javascript::LANGUAGE.into();
        parser.set_language(&language).expect("tree-sitter-javascript language load");

        for (name, expect_valid) in fixtures {
            let src = super::load_fixture(name);
            let wrapped = format!("function __spike__() {{ {} }}", src);
            let tree = match parser.parse(wrapped.as_bytes(), None) {
                Some(t) => t,
                None => {
                    println!("  {name}: PARSE_ERROR");
                    continue;
                }
            };

            let root = tree.root_node();
            // Navigate to function body
            let body_node = root
                .named_child(0) // function_declaration
                .and_then(|f| f.child_by_field_name("body")); // statement_block

            let mut walker = Walker::new(wrapped.as_bytes());
            if let Some(body) = body_node {
                let mut cursor = body.walk();
                for child in body.named_children(&mut cursor) {
                    walker.walk(child);
                }
            }

            let valid = walker.violations.is_empty();
            let status = if valid == *expect_valid { "OK" } else { "MISMATCH" };

            if valid {
                println!("  {name}: ACCEPT [{status}]");
                if !walker.ir_calls.is_empty() {
                    println!("    IR sequence: {:?}", walker.ir_calls);
                }
            } else {
                for (kind, sl, sc, el, ec) in &walker.violations {
                    println!(
                        "  {name}: REJECT [{status}] — {kind} at line {sl}:{sc}..{el}:{ec}"
                    );
                }
            }
        }
    }
}

// --- BOA -------------------------------------------------------------------
#[cfg(feature = "boa")]
mod boa_candidate {
    use boa_ast::{
        StatementListItem,
        declaration::Declaration,
        expression::{
            Expression,
            access::{PropertyAccess, PropertyAccessField},
            literal::PropertyDefinition,
        },
        scope::Scope,
        statement::Statement as BoaStmt,
        Spanned,
    };
    use boa_interner::{Interner, Sym};
    use boa_parser::{Parser, Source};

    struct Violation {
        kind: &'static str,
        line: u32,
        col: u32,
    }

    struct Walker<'i> {
        interner: &'i Interner,
        violations: Vec<Violation>,
        ir_calls: Vec<String>,
    }

    impl<'i> Walker<'i> {
        fn new(interner: &'i Interner) -> Self {
            Walker { interner, violations: Vec::new(), ir_calls: Vec::new() }
        }

        fn sym_str(&self, sym: Sym) -> &str {
            self.interner.resolve(sym).and_then(|r| r.utf8()).unwrap_or("<?>")
        }

        fn reject(&mut self, kind: &'static str, line: u32, col: u32) {
            self.violations.push(Violation { kind, line, col });
        }

        fn walk_stmts(&mut self, stmts: &[StatementListItem]) {
            for item in stmts {
                match item {
                    StatementListItem::Statement(s) => self.walk_stmt(s),
                    StatementListItem::Declaration(d) => self.walk_decl(d),
                }
            }
        }

        fn walk_decl(&mut self, decl: &Declaration) {
            match decl {
                Declaration::FunctionDeclaration(f) => {
                    let pos = f.name().span().start();
                    self.reject("function_declaration", pos.line_number(), pos.column_number());
                    self.walk_stmts(f.body().statements());
                }
                Declaration::GeneratorDeclaration(g) => {
                    let pos = g.name().span().start();
                    self.reject("generator_function", pos.line_number(), pos.column_number());
                    self.walk_stmts(g.body().statements());
                }
                Declaration::ClassDeclaration(c) => {
                    // ClassDeclaration has no Spanned impl in 0.21; use name span
                    let pos = c.name().span().start();
                    self.reject("class_declaration", pos.line_number(), pos.column_number());
                }
                Declaration::Lexical(lex) => {
                    for binding in lex.variable_list().as_ref() {
                        if let Some(init) = binding.init() {
                            self.walk_expr(init);
                        }
                    }
                }
                _ => {}
            }
        }

        fn walk_stmt(&mut self, stmt: &BoaStmt) {
            match stmt {
                BoaStmt::Var(v) => {
                    // VarDeclaration does NOT impl Spanned in boa 0.21 — use line 0
                    self.reject("var_declaration", 0, 0);
                    for binding in v.0.as_ref() {
                        if let Some(init) = binding.init() {
                            self.walk_expr(init);
                        }
                    }
                }
                BoaStmt::WhileLoop(w) => {
                    // WhileLoop does NOT impl Spanned in boa 0.21 — use line 0
                    self.reject("while_loop", 0, 0);
                    self.walk_expr(w.condition());
                    self.walk_stmt(w.body());
                }
                BoaStmt::Return(r) => {
                    if let Some(target) = r.target() {
                        self.walk_expr(target);
                    }
                }
                BoaStmt::Expression(e) => {
                    self.walk_expr(e);
                }
                BoaStmt::Block(b) => {
                    self.walk_stmts(b.statement_list().statements());
                }
                _ => {}
            }
        }

        fn walk_expr(&mut self, expr: &Expression) {
            match expr {
                Expression::PropertyAccess(pa) => {
                    if let PropertyAccess::Simple(spa) = pa {
                        match spa.field() {
                            PropertyAccessField::Expr(_) => {
                                // dynamic/computed: rollshot[k]
                                let pos = spa.target().span().start();
                                self.reject("dynamic_member_access", pos.line_number(), pos.column_number());
                            }
                            PropertyAccessField::Const(name) => {
                                let name_str = self.sym_str(name.sym()).to_owned();
                                if let Expression::Identifier(target) = spa.target() {
                                    let t = self.sym_str(target.sym()).to_owned();
                                    if t == "rollshot" {
                                        self.ir_calls.push(format!("rollshot.{name_str}"));
                                    }
                                    if t == "Reflect" || t == "Proxy" {
                                        let pos = spa.target().span().start();
                                        self.reject("reflection", pos.line_number(), pos.column_number());
                                    }
                                }
                                if matches!(name_str.as_str(), "filter" | "map" | "reduce" | "flatMap") {
                                    self.ir_calls.push(format!(".{name_str}"));
                                }
                                self.walk_expr(spa.target());
                            }
                        }
                    }
                }
                Expression::Call(call) => {
                    self.walk_expr(call.function());
                    for arg in call.args() {
                        self.walk_expr(arg);
                    }
                }
                Expression::ArrowFunction(arrow) => {
                    // Check body for escaping closure: outer_ident.push(...)
                    // Expression-body arrows: Boa represents `(x) => expr` as Return(expr)
                    for item in arrow.body().statements() {
                        if let StatementListItem::Statement(boxed_stmt) = item {
                            let call_expr = match boxed_stmt.as_ref() {
                                // statement-body arrow: { ident.push(x) }
                                BoaStmt::Expression(e) => Some(e),
                                // expression-body arrow: (x) => ident.push(x)
                                BoaStmt::Return(r) => r.target(),
                                _ => None,
                            };
                            if let Some(Expression::Call(call)) = call_expr {
                                if let Expression::PropertyAccess(PropertyAccess::Simple(spa)) =
                                    call.function()
                                {
                                    if let PropertyAccessField::Const(name) = spa.field() {
                                        let name_str = self.sym_str(name.sym()).to_owned();
                                        if name_str == "push" {
                                            if matches!(spa.target(), Expression::Identifier(_)) {
                                                self.reject("escaping_closure", 0, 0);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    self.walk_stmts(arrow.body().statements());
                }
                Expression::ObjectLiteral(obj) => {
                    for prop in obj.properties() {
                        if let PropertyDefinition::Property(_, val) = prop {
                            self.walk_expr(val);
                        }
                    }
                }
                Expression::ArrayLiteral(arr) => {
                    for el in arr.as_ref() {
                        if let Some(e) = el {
                            self.walk_expr(e);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    pub fn run() {
        println!("\n=== BOA CANDIDATE ===");
        println!("  NOTE: Boa AST 0.21 - VarDeclaration/WhileLoop lack Spanned; some spans show 0:0");

        let fixtures: &[(&str, bool)] = &[
            ("valid_detector.js", true),
            ("reject_var.js", false),
            ("reject_while.js", false),
            ("reject_dynamic_access.js", false),
            ("reject_reflect.js", false),
            ("reject_recursion.js", false),
            ("reject_class.js", false),
            ("reject_escaping_closure.js", false),
            ("reject_generator.js", false),
        ];

        for (name, expect_valid) in fixtures {
            let src = super::load_fixture(name);
            let wrapped = format!("function __spike__() {{ {} }}", src);
            let mut interner = Interner::new();
            let scope = Scope::default();
            let mut parser = Parser::new(Source::from_bytes(wrapped.as_bytes()));
            let script = match parser.parse_script(&scope, &mut interner) {
                Ok(s) => s,
                Err(e) => {
                    println!("  {name}: PARSE_ERROR - {e:?}");
                    continue;
                }
            };

            let mut walker = Walker::new(&interner);
            // Navigate into the outer wrapper function body
            let outer = script.statements().statements().first();
            if let Some(StatementListItem::Declaration(boxed_decl)) = outer {
                if let Declaration::FunctionDeclaration(outer_fn) = boxed_decl.as_ref() {
                    walker.walk_stmts(outer_fn.body().statements());
                }
            }

            let valid = walker.violations.is_empty();
            let status = if valid == *expect_valid { "OK" } else { "MISMATCH" };

            if valid {
                println!("  {name}: ACCEPT [{status}]");
                if !walker.ir_calls.is_empty() {
                    println!("    IR sequence: {:?}", walker.ir_calls);
                }
            } else {
                for v in &walker.violations {
                    println!(
                        "  {name}: REJECT [{status}] - {} (line {}:{})",
                        v.kind, v.line, v.col
                    );
                }
            }
        }
    }
}


fn main() {
    #[cfg(feature = "oxc")]
    oxc_candidate::run();

    #[cfg(feature = "swc")]
    swc_candidate::run();

    #[cfg(feature = "treesitter")]
    ts_candidate::run();

    #[cfg(feature = "boa")]
    boa_candidate::run();

    #[cfg(not(any(feature = "oxc", feature = "swc", feature = "treesitter", feature = "boa")))]
    println!("No feature enabled. Use: --features oxc|swc|treesitter|boa");
}
