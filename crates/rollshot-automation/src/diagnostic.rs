use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticCode {
    ParseError,
    MissingMain,
    DuplicateMain,
    InvalidMainSignature,
    InvalidTopLevel,
    UnsupportedSyntax,
    UnknownIdentifier,
    ForbiddenShadowing,
    HelperImpurity,
    RecursiveHelper,
    EscapingClosure,
    UnboundedCollection,
    DuplicateObjectKey,
    StaticLimitExceeded,
    NormalizationFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSpan {
    pub start_byte: u32,
    pub end_byte: u32,
    pub start_line: u32,
    pub start_column: u32,
    pub end_line: u32,
    pub end_column: u32,
}

impl SourceSpan {
    pub fn from_offsets(source: &str, start_byte: u32, end_byte: u32) -> Self {
        let locate = |offset: u32| {
            let prefix = &source.as_bytes()[..offset as usize];
            let line = prefix.iter().filter(|&&byte| byte == b'\n').count() as u32 + 1;
            let column = prefix
                .iter()
                .rposition(|&byte| byte == b'\n')
                .map(|index| prefix.len() - index)
                .unwrap_or(prefix.len() + 1) as u32;
            (line, column)
        };
        let (start_line, start_column) = locate(start_byte);
        let (end_line, end_column) = locate(end_byte);
        Self {
            start_byte,
            end_byte,
            start_line,
            start_column,
            end_line,
            end_column,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelatedDiagnostic {
    pub message: String,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceDiagnostic {
    pub code: DiagnosticCode,
    pub severity: DiagnosticSeverity,
    pub message: String,
    pub primary_span: SourceSpan,
    pub related: Vec<RelatedDiagnostic>,
}
