use oxc_ast::ast::{BindingPattern, FormalParameterKind, Function, Statement};
use oxc_span::GetSpan;

use crate::{DiagnosticCode, DiagnosticSeverity, SourceDiagnostic, SourceSpan, ValidationLimits};

pub(super) struct ShapeValidation {
    pub diagnostics: Vec<SourceDiagnostic>,
    #[allow(dead_code)]
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
