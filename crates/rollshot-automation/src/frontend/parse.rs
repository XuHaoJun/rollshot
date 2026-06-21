use oxc_allocator::Allocator;
use oxc_parser::{ParseOptions, Parser};
use oxc_span::SourceType;

use crate::{DiagnosticCode, DiagnosticSeverity, SourceDiagnostic, SourceSpan};

pub(super) fn with_program<T>(
    source: &str,
    use_program: impl for<'a> FnOnce(&oxc_ast::ast::Program<'a>) -> T,
) -> Result<T, Vec<SourceDiagnostic>> {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::default().with_script(true))
        .with_options(ParseOptions::default())
        .parse();

    if !parsed.diagnostics.is_empty() {
        return Err(parsed
            .diagnostics
            .into_iter()
            .map(|diagnostic| {
                let span = diagnostic.labels.as_slice().first().map_or(
                    SourceSpan::from_offsets(source, 0, source.len() as u32),
                    |label| {
                        let start = label.offset();
                        SourceSpan::from_offsets(source, start, start + label.len())
                    },
                );
                SourceDiagnostic {
                    code: DiagnosticCode::ParseError,
                    severity: DiagnosticSeverity::Error,
                    message: diagnostic.message.to_string(),
                    primary_span: span,
                    related: Vec::new(),
                }
            })
            .collect());
    }

    Ok(use_program(&parsed.program))
}
