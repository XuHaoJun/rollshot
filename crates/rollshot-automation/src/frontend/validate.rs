use std::collections::{BTreeMap, BTreeSet};

use oxc_ast::ast::{
    self, Argument, ArrowFunctionExpression, BindingPattern, CallExpression, Expression,
    FormalParameterKind, Function, ObjectPropertyKind, PropertyKey, Statement,
    VariableDeclarationKind,
};
use oxc_span::GetSpan;

use crate::{DiagnosticCode, DiagnosticSeverity, SourceDiagnostic, SourceSpan, ValidationLimits};

pub(super) struct ShapeValidation {
    pub diagnostics: Vec<SourceDiagnostic>,
    pub function_names: Vec<String>,
}

pub(super) fn validate_shape(
    source: &str,
    program: &oxc_ast::ast::Program<'_>,
    limits: &ValidationLimits,
) -> ShapeValidation {
    let mut diagnostics = Vec::new();
    let mut function_names = Vec::new();
    let mut main_count = 0_u32;

    if source.len() > limits.max_source_bytes {
        diagnostics.push(error(
            source,
            DiagnosticCode::StaticLimitExceeded,
            0,
            source.len() as u32,
            "source exceeds the configured byte limit",
        ));
    }

    for statement in &program.body {
        let Statement::FunctionDeclaration(function) = statement else {
            let span = statement.span();
            diagnostics.push(error(
                source,
                DiagnosticCode::InvalidTopLevel,
                span.start,
                span.end,
                "top level may contain only named function declarations",
            ));
            continue;
        };

        let Some(id) = &function.id else {
            diagnostics.push(error(
                source,
                DiagnosticCode::InvalidTopLevel,
                function.span.start,
                function.span.end,
                "top-level functions must be named",
            ));
            continue;
        };
        function_names.push(id.name.to_string());

        if id.name == "main" {
            main_count += 1;
            validate_main(source, function, &mut diagnostics);
        } else {
            validate_helper_signature(source, function, &mut diagnostics);
        }
    }

    match main_count {
        0 => diagnostics.push(error(
            source,
            DiagnosticCode::MissingMain,
            0,
            source.len() as u32,
            "define exactly one synchronous function main(input)",
        )),
        1 => {}
        _ => diagnostics.push(error(
            source,
            DiagnosticCode::DuplicateMain,
            0,
            source.len() as u32,
            "define only one function named main",
        )),
    }

    if function_names.len().saturating_sub(1) > limits.max_helpers as usize {
        diagnostics.push(error(
            source,
            DiagnosticCode::StaticLimitExceeded,
            0,
            source.len() as u32,
            "helper count exceeds the configured limit",
        ));
    }

    ShapeValidation {
        diagnostics,
        function_names,
    }
}

fn validate_main(source: &str, function: &Function<'_>, diagnostics: &mut Vec<SourceDiagnostic>) {
    let parameter_is_input = function.params.kind == FormalParameterKind::FormalParameter
        && function.params.items.len() == 1
        && matches!(
            &function.params.items[0].pattern,
            BindingPattern::BindingIdentifier(identifier) if identifier.name == "input"
        );
    let body_has_final_return = function.body.as_ref().is_some_and(|body| {
        body.statements.len() == 1
            && matches!(body.statements.last(), Some(Statement::ReturnStatement(_)))
            || body.statements.len() > 1
                && matches!(body.statements.last(), Some(Statement::ReturnStatement(_)))
                && body
                    .statements
                    .iter()
                    .filter(|statement| matches!(statement, Statement::ReturnStatement(_)))
                    .count()
                    == 1
    });

    if function.r#async || function.generator || !parameter_is_input || !body_has_final_return {
        diagnostics.push(error(
            source,
            DiagnosticCode::InvalidMainSignature,
            function.span.start,
            function.span.end,
            "main must be synchronous function main(input) with one final top-level return",
        ));
    }
}

fn validate_helper_signature(
    source: &str,
    function: &Function<'_>,
    diagnostics: &mut Vec<SourceDiagnostic>,
) {
    if function.r#async || function.generator || function.body.is_none() {
        diagnostics.push(error(
            source,
            DiagnosticCode::UnsupportedSyntax,
            function.span.start,
            function.span.end,
            "helpers must be synchronous non-generator functions with bodies",
        ));
    }
    if function.params.rest.is_some() {
        diagnostics.push(error(
            source,
            DiagnosticCode::UnsupportedSyntax,
            function.span.start,
            function.span.end,
            "helpers must not use rest parameters",
        ));
    }
    for param in &function.params.items {
        if !matches!(param.pattern, BindingPattern::BindingIdentifier(_)) {
            diagnostics.push(error(
                source,
                DiagnosticCode::UnsupportedSyntax,
                param.span.start,
                param.span.end,
                "helper parameters must be simple identifiers",
            ));
        }
        if param.initializer.is_some() {
            diagnostics.push(error(
                source,
                DiagnosticCode::UnsupportedSyntax,
                param.span.start,
                param.span.end,
                "helper parameters must not have default values",
            ));
        }
    }
}

const ALLOWED_ROLLSHOT_CAPABILITIES: &[&str] =
    &["ocr", "layout", "regionFeatures", "templateMatch"];
const ALLOWED_MATH_METHODS: &[&str] = &[
    "abs", "ceil", "floor", "round", "trunc", "min", "max", "sqrt", "hypot",
];
const ALLOWED_ARRAY_CALLBACK_METHODS: &[&str] = &["map", "filter", "some", "every"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FunctionKind {
    Main,
    Helper,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArrowPosition {
    NotCallback,
    ArrayCallback,
}

pub(super) struct FunctionFacts {
    pub name: String,
    pub calls: Vec<(String, oxc_span::Span)>,
}

pub(super) struct ValidationFacts {
    pub ast_nodes: u32,
    pub literal_bytes: usize,
    pub functions: Vec<FunctionFacts>,
}

struct BodyValidator<'a> {
    source: &'a str,
    helper_names: BTreeSet<String>,
    diagnostics: Vec<SourceDiagnostic>,
    facts: Vec<FunctionFacts>,
    ast_nodes: u32,
    literal_bytes: usize,
}

impl<'a> BodyValidator<'a> {
    fn new(source: &'a str, helper_names: BTreeSet<String>) -> Self {
        Self {
            source,
            helper_names,
            diagnostics: Vec::new(),
            facts: Vec::new(),
            ast_nodes: 0,
            literal_bytes: 0,
        }
    }

    fn visit_function(&mut self, name: &str, function: &Function<'a>, kind: FunctionKind) {
        let params: BTreeSet<String> = function
            .params
            .items
            .iter()
            .filter_map(|param| {
                if let BindingPattern::BindingIdentifier(id) = &param.pattern {
                    Some(id.name.to_string())
                } else {
                    None
                }
            })
            .collect();

        let mut locals = BTreeSet::new();
        let mut facts = FunctionFacts {
            name: name.to_string(),
            calls: Vec::new(),
        };

        if let Some(body) = &function.body {
            for stmt in &body.statements {
                self.visit_statement(stmt, kind, &params, &mut locals, &mut facts);
            }
        }

        self.facts.push(facts);
    }

    fn visit_statement(
        &mut self,
        stmt: &Statement<'a>,
        kind: FunctionKind,
        params: &BTreeSet<String>,
        locals: &mut BTreeSet<String>,
        facts: &mut FunctionFacts,
    ) {
        self.ast_nodes += 1;
        match stmt {
            Statement::VariableDeclaration(decl) => {
                if decl.kind != VariableDeclarationKind::Const {
                    self.emit(
                        DiagnosticCode::UnsupportedSyntax,
                        decl.span.start,
                        decl.span.end,
                        "only const declarations are allowed",
                    );
                    return;
                }
                for declarator in &decl.declarations {
                    self.ast_nodes += 1;
                    if let Some(init) = &declarator.init {
                        self.visit_expression(
                            init,
                            kind,
                            params,
                            locals,
                            facts,
                            ArrowPosition::NotCallback,
                        );
                    }
                    match &declarator.id {
                        BindingPattern::BindingIdentifier(id) => {
                            locals.insert(id.name.to_string());
                        }
                        _ => {
                            self.emit(
                                DiagnosticCode::UnsupportedSyntax,
                                declarator.span.start,
                                declarator.span.end,
                                "destructuring in declarations is not allowed",
                            );
                        }
                    }
                }
            }
            Statement::ReturnStatement(ret) => {
                if let Some(arg) = &ret.argument {
                    self.visit_expression(
                        arg,
                        kind,
                        params,
                        locals,
                        facts,
                        ArrowPosition::NotCallback,
                    );
                }
            }
            Statement::ExpressionStatement(expr_stmt) => {
                self.visit_expression(
                    &expr_stmt.expression,
                    kind,
                    params,
                    locals,
                    facts,
                    ArrowPosition::NotCallback,
                );
            }
            // Default-deny catch-all for all other statement kinds
            _ => {
                self.emit(
                    DiagnosticCode::UnsupportedSyntax,
                    stmt.span().start,
                    stmt.span().end,
                    "unsupported statement kind",
                );
            }
        }
    }

    fn visit_expression(
        &mut self,
        expr: &Expression<'a>,
        kind: FunctionKind,
        params: &BTreeSet<String>,
        locals: &BTreeSet<String>,
        facts: &mut FunctionFacts,
        arrow_pos: ArrowPosition,
    ) {
        self.ast_nodes += 1;
        match expr {
            Expression::StringLiteral(lit) => {
                self.literal_bytes += lit.value.len();
            }
            Expression::NumericLiteral(lit) => {
                self.literal_bytes += lit.value.to_string().len();
            }
            Expression::BooleanLiteral(_)
            | Expression::NullLiteral(_)
            | Expression::BigIntLiteral(_)
            | Expression::RegExpLiteral(_) => {}

            Expression::Identifier(ident) => {
                let name = ident.name.as_str();
                let allowed = match kind {
                    FunctionKind::Main => {
                        params.contains(name)
                            || locals.contains(name)
                            || name == "input"
                            || name == "rollshot"
                            || name == "Math"
                            || self.helper_names.contains(name)
                    }
                    FunctionKind::Helper => {
                        params.contains(name)
                            || locals.contains(name)
                            || name == "Math"
                            || self.helper_names.contains(name)
                    }
                };
                if !allowed {
                    self.emit(
                        DiagnosticCode::UnknownIdentifier,
                        ident.span.start,
                        ident.span.end,
                        &format!("unknown identifier {name}"),
                    );
                }
            }

            Expression::StaticMemberExpression(member) => {
                self.visit_expression(
                    &member.object,
                    kind,
                    params,
                    locals,
                    facts,
                    ArrowPosition::NotCallback,
                );
            }

            Expression::CallExpression(call) => {
                self.visit_call(call, kind, params, locals, facts);
            }

            Expression::ArrowFunctionExpression(arrow) => {
                if arrow_pos == ArrowPosition::ArrayCallback {
                    self.visit_arrow_body(arrow, kind, params, locals, facts);
                } else {
                    self.emit(
                        DiagnosticCode::EscapingClosure,
                        arrow.span.start,
                        arrow.span.end,
                        "arrow functions are only allowed as immediate callbacks to .map/.filter/.some/.every",
                    );
                }
            }

            Expression::ObjectExpression(obj) => {
                let mut keys: BTreeMap<String, u32> = BTreeMap::new();
                for prop_kind in &obj.properties {
                    self.ast_nodes += 1;
                    match prop_kind {
                        ObjectPropertyKind::SpreadProperty(spread) => {
                            self.emit(
                                DiagnosticCode::UnsupportedSyntax,
                                spread.span.start,
                                spread.span.end,
                                "spread in object literals is not allowed",
                            );
                        }
                        ObjectPropertyKind::ObjectProperty(prop) => {
                            if prop.kind != ast::PropertyKind::Init {
                                self.emit(
                                    DiagnosticCode::UnsupportedSyntax,
                                    prop.span.start,
                                    prop.span.end,
                                    "getter/setter properties are not allowed",
                                );
                                continue;
                            }
                            if prop.computed {
                                self.emit(
                                    DiagnosticCode::UnsupportedSyntax,
                                    prop.span.start,
                                    prop.span.end,
                                    "computed property keys are not allowed",
                                );
                                continue;
                            }
                            if prop.method {
                                self.emit(
                                    DiagnosticCode::UnsupportedSyntax,
                                    prop.span.start,
                                    prop.span.end,
                                    "method shorthand is not allowed",
                                );
                                continue;
                            }
                            if prop.shorthand {
                                self.emit(
                                    DiagnosticCode::UnsupportedSyntax,
                                    prop.span.start,
                                    prop.span.end,
                                    "shorthand properties are not allowed",
                                );
                                continue;
                            }
                            if let PropertyKey::StaticIdentifier(key) = &prop.key {
                                let key_str = key.name.to_string();
                                if let Some(&first_span) = keys.get(&key_str) {
                                    self.diagnostics.push(SourceDiagnostic {
                                        code: DiagnosticCode::DuplicateObjectKey,
                                        severity: DiagnosticSeverity::Error,
                                        message: format!("duplicate object key {key_str:?}"),
                                        primary_span: SourceSpan::from_offsets(
                                            self.source,
                                            prop.span.start,
                                            prop.span.end,
                                        ),
                                        related: vec![crate::RelatedDiagnostic {
                                            message: "first occurrence".into(),
                                            span: SourceSpan::from_offsets(
                                                self.source,
                                                first_span,
                                                first_span,
                                            ),
                                        }],
                                    });
                                } else {
                                    keys.insert(key_str, prop.span.start);
                                }
                            } else {
                                self.emit(
                                    DiagnosticCode::UnsupportedSyntax,
                                    prop.span.start,
                                    prop.span.end,
                                    "unsupported property key kind",
                                );
                                continue;
                            }
                            self.visit_expression(
                                &prop.value,
                                kind,
                                params,
                                locals,
                                facts,
                                ArrowPosition::NotCallback,
                            );
                        }
                    }
                }
            }

            Expression::ArrayExpression(arr) => {
                for element in &arr.elements {
                    self.ast_nodes += 1;
                    match element {
                        ast::ArrayExpressionElement::SpreadElement(spread) => {
                            self.emit(
                                DiagnosticCode::UnsupportedSyntax,
                                spread.span.start,
                                spread.span.end,
                                "spread in array literals is not allowed",
                            );
                        }
                        ast::ArrayExpressionElement::Elision(_) => {
                            self.emit(
                                DiagnosticCode::UnsupportedSyntax,
                                arr.span.start,
                                arr.span.end,
                                "elided array elements are not allowed",
                            );
                        }
                        _ => {
                            if let Some(expr) = element.as_expression() {
                                self.visit_expression(
                                    expr,
                                    kind,
                                    params,
                                    locals,
                                    facts,
                                    ArrowPosition::NotCallback,
                                );
                            }
                        }
                    }
                }
            }

            Expression::BinaryExpression(binary) => {
                self.visit_expression(
                    &binary.left,
                    kind,
                    params,
                    locals,
                    facts,
                    ArrowPosition::NotCallback,
                );
                self.visit_expression(
                    &binary.right,
                    kind,
                    params,
                    locals,
                    facts,
                    ArrowPosition::NotCallback,
                );
            }

            Expression::UnaryExpression(unary) => {
                self.visit_expression(
                    &unary.argument,
                    kind,
                    params,
                    locals,
                    facts,
                    ArrowPosition::NotCallback,
                );
            }

            Expression::LogicalExpression(logical) => {
                self.visit_expression(
                    &logical.left,
                    kind,
                    params,
                    locals,
                    facts,
                    ArrowPosition::NotCallback,
                );
                self.visit_expression(
                    &logical.right,
                    kind,
                    params,
                    locals,
                    facts,
                    ArrowPosition::NotCallback,
                );
            }

            Expression::ConditionalExpression(cond) => {
                self.visit_expression(
                    &cond.test,
                    kind,
                    params,
                    locals,
                    facts,
                    ArrowPosition::NotCallback,
                );
                self.visit_expression(
                    &cond.consequent,
                    kind,
                    params,
                    locals,
                    facts,
                    ArrowPosition::NotCallback,
                );
                self.visit_expression(
                    &cond.alternate,
                    kind,
                    params,
                    locals,
                    facts,
                    ArrowPosition::NotCallback,
                );
            }

            Expression::ParenthesizedExpression(paren) => {
                self.visit_expression(
                    &paren.expression,
                    kind,
                    params,
                    locals,
                    facts,
                    ArrowPosition::NotCallback,
                );
            }

            Expression::SequenceExpression(seq) => {
                for e in &seq.expressions {
                    self.visit_expression(
                        e,
                        kind,
                        params,
                        locals,
                        facts,
                        ArrowPosition::NotCallback,
                    );
                }
            }

            Expression::TemplateLiteral(_) => {
                self.emit(
                    DiagnosticCode::UnsupportedSyntax,
                    expr.span().start,
                    expr.span().end,
                    "template literals are not allowed",
                );
            }

            // Everything else: default-deny catch-all
            Expression::AssignmentExpression(_)
            | Expression::AwaitExpression(_)
            | Expression::ChainExpression(_)
            | Expression::ClassExpression(_)
            | Expression::FunctionExpression(_)
            | Expression::ImportExpression(_)
            | Expression::MetaProperty(_)
            | Expression::NewExpression(_)
            | Expression::Super(_)
            | Expression::TaggedTemplateExpression(_)
            | Expression::ThisExpression(_)
            | Expression::UpdateExpression(_)
            | Expression::YieldExpression(_)
            | Expression::PrivateInExpression(_)
            | Expression::ComputedMemberExpression(_)
            | Expression::JSXElement(_)
            | Expression::JSXFragment(_) => {
                self.emit(
                    DiagnosticCode::UnsupportedSyntax,
                    expr.span().start,
                    expr.span().end,
                    "unsupported expression kind",
                );
            }

            // oxc uses `inherit_variants!` so TS variants are part of Expression
            _ => {
                self.emit(
                    DiagnosticCode::UnsupportedSyntax,
                    expr.span().start,
                    expr.span().end,
                    "unsupported expression kind",
                );
            }
        }
    }

    fn visit_call(
        &mut self,
        call: &CallExpression<'a>,
        kind: FunctionKind,
        params: &BTreeSet<String>,
        locals: &BTreeSet<String>,
        facts: &mut FunctionFacts,
    ) {
        self.ast_nodes += 1;

        match &call.callee {
            Expression::StaticMemberExpression(member) => {
                let method_name = member.property.name.as_str();

                // Check for .call / .apply
                if method_name == "call" || method_name == "apply" {
                    self.emit(
                        DiagnosticCode::UnsupportedSyntax,
                        call.span.start,
                        call.span.end,
                        ".call() and .apply() are not allowed",
                    );
                    return;
                }

                match &member.object {
                    Expression::Identifier(obj) => {
                        let obj_name = obj.name.as_str();

                        if obj_name == "rollshot" {
                            if kind == FunctionKind::Helper {
                                self.emit(
                                    DiagnosticCode::HelperImpurity,
                                    call.span.start,
                                    call.span.end,
                                    "helpers must not call capabilities",
                                );
                                return;
                            }
                            if !ALLOWED_ROLLSHOT_CAPABILITIES.contains(&method_name) {
                                self.emit(
                                    DiagnosticCode::UnsupportedSyntax,
                                    call.span.start,
                                    call.span.end,
                                    &format!(
                                        "rollshot.{method_name} is not a supported capability"
                                    ),
                                );
                                return;
                            }
                            self.visit_arguments(&call.arguments, kind, params, locals, facts);
                        } else if obj_name == "Math" {
                            if ALLOWED_MATH_METHODS.contains(&method_name) {
                                self.visit_arguments(&call.arguments, kind, params, locals, facts);
                            } else {
                                self.emit(
                                    DiagnosticCode::UnsupportedSyntax,
                                    call.span.start,
                                    call.span.end,
                                    &format!("Math.{method_name} is not allowed"),
                                );
                                self.visit_arguments(&call.arguments, kind, params, locals, facts);
                            }
                        } else if self.helper_names.contains(obj_name) {
                            self.emit(
                                DiagnosticCode::UnsupportedSyntax,
                                call.span.start,
                                call.span.end,
                                "only direct helper calls are allowed",
                            );
                        } else if params.contains(obj_name) || locals.contains(obj_name) {
                            // Local variable method call
                            if ALLOWED_ARRAY_CALLBACK_METHODS.contains(&method_name) {
                                self.visit_arguments_with_arrow_pos(
                                    &call.arguments,
                                    kind,
                                    params,
                                    locals,
                                    facts,
                                    ArrowPosition::ArrayCallback,
                                );
                            } else {
                                self.emit(
                                    DiagnosticCode::UnsupportedSyntax,
                                    call.span.start,
                                    call.span.end,
                                    &format!("method .{method_name}() is not allowed"),
                                );
                                self.visit_arguments(&call.arguments, kind, params, locals, facts);
                            }
                        } else {
                            // Unknown object - emit UnknownIdentifier for the object
                            self.visit_expression(
                                &member.object,
                                kind,
                                params,
                                locals,
                                facts,
                                ArrowPosition::NotCallback,
                            );
                            self.visit_arguments(&call.arguments, kind, params, locals, facts);
                        }
                    }
                    Expression::CallExpression(inner_call) => {
                        // Chained call: fn().method(...)
                        self.visit_call(inner_call, kind, params, locals, facts);
                        if ALLOWED_ARRAY_CALLBACK_METHODS.contains(&method_name) {
                            self.visit_arguments_with_arrow_pos(
                                &call.arguments,
                                kind,
                                params,
                                locals,
                                facts,
                                ArrowPosition::ArrayCallback,
                            );
                        } else {
                            // Disallowed method on chained result
                            // Emit UnsupportedSyntax before visiting arguments
                            self.emit(
                                DiagnosticCode::UnsupportedSyntax,
                                call.span.start,
                                call.span.end,
                                &format!("method .{method_name}() is not allowed"),
                            );
                            // Still visit arguments to catch nested errors
                            self.visit_arguments(&call.arguments, kind, params, locals, facts);
                        }
                    }
                    _ => {
                        // Other callee base (e.g. ArrayExpression for [].sort(), [].reduce(...))
                        // Visit the callee object first
                        self.visit_expression(
                            &member.object,
                            kind,
                            params,
                            locals,
                            facts,
                            ArrowPosition::NotCallback,
                        );
                        if ALLOWED_ARRAY_CALLBACK_METHODS.contains(&method_name) {
                            self.visit_arguments_with_arrow_pos(
                                &call.arguments,
                                kind,
                                params,
                                locals,
                                facts,
                                ArrowPosition::ArrayCallback,
                            );
                        } else {
                            // Disallowed method
                            // Emit UnsupportedSyntax before visiting arguments
                            self.emit(
                                DiagnosticCode::UnsupportedSyntax,
                                call.span.start,
                                call.span.end,
                                &format!("method .{method_name}() is not allowed"),
                            );
                            // Still visit arguments to catch nested errors
                            self.visit_arguments(&call.arguments, kind, params, locals, facts);
                        }
                    }
                }
            }
            Expression::Identifier(ident) => {
                let name = ident.name.as_str();
                if self.helper_names.contains(name) {
                    facts.calls.push((name.to_string(), call.span));
                    self.visit_arguments(&call.arguments, kind, params, locals, facts);
                } else {
                    self.emit(
                        DiagnosticCode::UnknownIdentifier,
                        ident.span.start,
                        ident.span.end,
                        &format!("unknown identifier {name}"),
                    );
                    self.visit_arguments(&call.arguments, kind, params, locals, facts);
                }
            }
            Expression::ComputedMemberExpression(computed) => {
                self.emit(
                    DiagnosticCode::UnsupportedSyntax,
                    computed.span.start,
                    computed.span.end,
                    "computed member access is not allowed",
                );
            }
            Expression::ChainExpression(chain) => {
                self.emit(
                    DiagnosticCode::UnsupportedSyntax,
                    chain.span.start,
                    chain.span.end,
                    "optional chaining is not allowed",
                );
            }
            Expression::CallExpression(inner_call) => {
                // Callee is itself a call: e.g. Function('return 1')()
                // Visit inner call to validate it (may emit UnknownIdentifier)
                self.visit_call(inner_call, kind, params, locals, facts);
                // The outer call uses an unsupported callee
                self.emit(
                    DiagnosticCode::UnknownIdentifier,
                    call.span.start,
                    call.span.end,
                    "unsupported call expression used as callee",
                );
            }
            _ => {
                self.emit(
                    DiagnosticCode::UnsupportedSyntax,
                    call.span.start,
                    call.span.end,
                    "unsupported call callee",
                );
            }
        }
    }

    fn visit_arguments(
        &mut self,
        arguments: &[Argument<'a>],
        kind: FunctionKind,
        params: &BTreeSet<String>,
        locals: &BTreeSet<String>,
        facts: &mut FunctionFacts,
    ) {
        self.visit_arguments_with_arrow_pos(
            arguments,
            kind,
            params,
            locals,
            facts,
            ArrowPosition::NotCallback,
        );
    }

    fn visit_arguments_with_arrow_pos(
        &mut self,
        arguments: &[Argument<'a>],
        kind: FunctionKind,
        params: &BTreeSet<String>,
        locals: &BTreeSet<String>,
        facts: &mut FunctionFacts,
        arrow_pos: ArrowPosition,
    ) {
        for arg in arguments {
            self.ast_nodes += 1;
            match arg {
                Argument::SpreadElement(spread) => {
                    self.emit(
                        DiagnosticCode::UnsupportedSyntax,
                        spread.span.start,
                        spread.span.end,
                        "spread arguments are not allowed",
                    );
                }
                _ => {
                    if let Some(expr) = arg.as_expression() {
                        self.visit_expression(expr, kind, params, locals, facts, arrow_pos);
                    }
                }
            }
        }
    }

    fn visit_arrow_body(
        &mut self,
        arrow: &ArrowFunctionExpression<'a>,
        kind: FunctionKind,
        params: &BTreeSet<String>,
        locals: &BTreeSet<String>,
        facts: &mut FunctionFacts,
    ) {
        self.ast_nodes += 1;
        if arrow.params.rest.is_some() {
            self.emit(
                DiagnosticCode::UnsupportedSyntax,
                arrow.span.start,
                arrow.span.end,
                "arrow function rest parameters are not allowed",
            );
        }
        // Track arrow parameters as locals
        let mut arrow_locals = locals.clone();
        for param in &arrow.params.items {
            self.ast_nodes += 1;
            if let BindingPattern::BindingIdentifier(id) = &param.pattern {
                arrow_locals.insert(id.name.to_string());
            } else {
                self.emit(
                    DiagnosticCode::UnsupportedSyntax,
                    param.span.start,
                    param.span.end,
                    "arrow function parameters must be simple identifiers",
                );
            }
        }
        for stmt in &arrow.body.statements {
            self.visit_statement(stmt, kind, params, &mut arrow_locals, facts);
        }
    }

    fn emit(&mut self, code: DiagnosticCode, start: u32, end: u32, message: &str) {
        self.diagnostics.push(SourceDiagnostic {
            code,
            severity: DiagnosticSeverity::Error,
            message: message.into(),
            primary_span: SourceSpan::from_offsets(self.source, start, end),
            related: Vec::new(),
        });
    }
}

fn detect_cycles(source: &str, facts: &[FunctionFacts], diagnostics: &mut Vec<SourceDiagnostic>) {
    let graph: BTreeMap<_, _> = facts
        .iter()
        .map(|fact| {
            (
                fact.name.as_str(),
                fact.calls
                    .iter()
                    .map(|(name, span)| (name.as_str(), *span))
                    .collect::<Vec<_>>(),
            )
        })
        .collect();

    fn visit<'a>(
        node: &'a str,
        graph: &BTreeMap<&'a str, Vec<(&'a str, oxc_span::Span)>>,
        visiting: &mut Vec<&'a str>,
        visited: &mut BTreeSet<&'a str>,
        source: &str,
        diagnostics: &mut Vec<SourceDiagnostic>,
    ) {
        if visited.contains(node) {
            return;
        }
        visiting.push(node);
        if let Some(edges) = graph.get(node) {
            for (next, span) in edges {
                if visiting.contains(next) {
                    diagnostics.push(SourceDiagnostic {
                        code: DiagnosticCode::RecursiveHelper,
                        severity: DiagnosticSeverity::Error,
                        message: format!("helper recursion cycle reaches {next}"),
                        primary_span: SourceSpan::from_offsets(source, span.start, span.end),
                        related: Vec::new(),
                    });
                } else {
                    visit(next, graph, visiting, visited, source, diagnostics);
                }
            }
        }
        visiting.pop();
        visited.insert(node);
    }

    let mut visited = BTreeSet::new();
    for node in graph.keys().copied() {
        visit(
            node,
            &graph,
            &mut Vec::new(),
            &mut visited,
            source,
            diagnostics,
        );
    }
}

pub(super) fn validate_bodies(
    source: &str,
    program: &oxc_ast::ast::Program<'_>,
    function_names: &[String],
    limits: &ValidationLimits,
) -> (Vec<SourceDiagnostic>, ValidationFacts) {
    let helper_names: BTreeSet<String> = function_names
        .iter()
        .filter(|name| name.as_str() != "main")
        .cloned()
        .collect();

    let mut validator = BodyValidator::new(source, helper_names);

    for statement in &program.body {
        if let Statement::FunctionDeclaration(function) = statement {
            let Some(id) = &function.id else {
                continue;
            };
            let kind = if id.name == "main" {
                FunctionKind::Main
            } else {
                FunctionKind::Helper
            };
            validator.visit_function(id.name.as_str(), function, kind);
        }
    }

    if validator.ast_nodes > limits.max_ast_nodes {
        validator.emit(
            DiagnosticCode::StaticLimitExceeded,
            0,
            source.len() as u32,
            "AST node count exceeds the configured limit",
        );
    }
    if validator.literal_bytes > limits.max_literal_bytes {
        validator.emit(
            DiagnosticCode::StaticLimitExceeded,
            0,
            source.len() as u32,
            "literal bytes exceed the configured limit",
        );
    }

    detect_cycles(source, &validator.facts, &mut validator.diagnostics);

    let facts = ValidationFacts {
        ast_nodes: validator.ast_nodes,
        literal_bytes: validator.literal_bytes,
        functions: validator.facts,
    };

    (validator.diagnostics, facts)
}

fn error(
    source: &str,
    code: DiagnosticCode,
    start: u32,
    end: u32,
    message: &str,
) -> SourceDiagnostic {
    SourceDiagnostic {
        code,
        severity: DiagnosticSeverity::Error,
        message: message.into(),
        primary_span: SourceSpan::from_offsets(source, start, end),
        related: Vec::new(),
    }
}
