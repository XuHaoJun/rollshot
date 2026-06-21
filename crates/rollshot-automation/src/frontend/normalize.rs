use std::collections::{BTreeMap, BTreeSet};

use oxc_ast::ast::{
    self, Argument, ArrowFunctionExpression, BindingPattern, CallExpression, Expression,
    ObjectPropertyKind, PropertyKey, Statement,
};
use oxc_span::GetSpan;

use crate::{
    CapabilityCallManifest, CapabilityManifest, CapabilityName, DiagnosticCode, DiagnosticSeverity,
    IrNodeKind, ProposedEditKind, SourceDiagnostic, SourceSpan, ValidationLimits,
    CAPABILITY_API_V1, IR_SCHEMA_V1,
};

use super::validate::ValidationFacts;
use crate::ir::*;

const ROLLSHOT_CAPABILITIES: &[(&str, CapabilityName)] = &[
    ("ocr", CapabilityName::Ocr),
    ("layout", CapabilityName::Layout),
    ("regionFeatures", CapabilityName::RegionFeatures),
    ("templateMatch", CapabilityName::TemplateMatch),
];
const MAX_CAPABILITY_RESULTS_PER_CALL: f64 = 1_000.0;

type CollectionKindFactory = fn(CollectionIr) -> IrNodeKind;

const COLLECTION_METHODS: &[(&str, CollectionKindFactory)] = &[
    ("map", IrNodeKind::CollectionMap),
    ("filter", IrNodeKind::CollectionFilter),
    ("some", IrNodeKind::CollectionSome),
    ("every", IrNodeKind::CollectionEvery),
];

struct FunctionNorm {
    name: String,
    source_span: SourceSpan,
    helper_call_depth: u32,
}

struct Normalizer<'a> {
    source: &'a str,
    limits: &'a ValidationLimits,
    helper_names: BTreeSet<String>,
    helper_functions: BTreeMap<String, &'a ast::Function<'a>>,
    next_node_id: NodeId,
    functions: Vec<FunctionNorm>,
    nodes: Vec<IrNode>,
    output: NodeId,
    capability_calls: Vec<CapabilityCallManifest>,
    max_output_candidates: u32,
    max_collection_traversals: u32,
    possible_edit_kinds: BTreeSet<ProposedEditKind>,
    unbounded_nodes: BTreeSet<NodeId>,
    diagnostics: Vec<SourceDiagnostic>,
}

pub(super) fn normalize(
    source: &str,
    program: &oxc_ast::ast::Program<'_>,
    facts: &ValidationFacts,
    limits: &ValidationLimits,
) -> Result<WorkflowIr, Vec<SourceDiagnostic>> {
    let mut main_index = None;
    let mut helper_names = BTreeSet::new();
    let mut func_order = Vec::new();

    for (i, stmt) in program.body.iter().enumerate() {
        if let Statement::FunctionDeclaration(function) = stmt {
            if let Some(id) = &function.id {
                func_order.push((id.name.to_string(), function));
                if id.name == "main" {
                    main_index = Some(i);
                } else {
                    helper_names.insert(id.name.to_string());
                }
            }
        }
    }

    let _main_index =
        main_index.ok_or_else(|| vec![missing_main_diagnostic(source, source.len() as u32)])?;

    let max_depth = compute_max_call_depth(&facts.functions);
    if max_depth > limits.max_helper_call_depth {
        return Err(vec![SourceDiagnostic {
            code: DiagnosticCode::StaticLimitExceeded,
            severity: DiagnosticSeverity::Error,
            message: "helper call depth exceeds the configured limit".into(),
            primary_span: SourceSpan::from_offsets(source, 0, source.len() as u32),
            related: Vec::new(),
        }]);
    }

    let mut normalizer = Normalizer {
        source,
        limits,
        helper_names,
        helper_functions: func_order
            .iter()
            .filter(|(name, _)| name != "main")
            .map(|(name, function)| (name.clone(), function.as_ref()))
            .collect(),
        next_node_id: 1,
        functions: Vec::new(),
        nodes: Vec::new(),
        output: 0,
        capability_calls: Vec::new(),
        max_output_candidates: 0,
        max_collection_traversals: 0,
        possible_edit_kinds: BTreeSet::new(),
        unbounded_nodes: BTreeSet::new(),
        diagnostics: Vec::new(),
    };

    for (func_index, (name, function)) in func_order.iter().enumerate() {
        let span = SourceSpan::from_offsets(source, function.span.start, function.span.end);
        let depth = facts
            .functions
            .iter()
            .find(|f| f.name == *name)
            .map(|f| compute_depth_for(&f.name, &facts.functions, &mut BTreeSet::new()))
            .unwrap_or(0);

        let func_norm = FunctionNorm {
            name: name.to_string(),
            source_span: span,
            helper_call_depth: depth,
        };
        normalizer.functions.push(func_norm);

        if let Some(body) = &function.body {
            let mut var_nodes: BTreeMap<String, NodeId> = BTreeMap::new();
            for stmt in &body.statements {
                if !matches!(stmt, Statement::ReturnStatement(_)) {
                    normalizer.normalize_statement(stmt, &mut var_nodes);
                }
            }

            let func_id = func_index as FunctionId;
            if let Some(Statement::ReturnStatement(ret)) = body.statements.last() {
                if let Some(arg) = &ret.argument {
                    let (output_id, cardinality) = normalizer.normalize_expression(arg, &var_nodes);
                    if normalizer.functions[func_id as usize].name == "main" {
                        normalizer.output = output_id;
                        normalizer.max_output_candidates = cardinality;
                    }
                }
            }
        }
    }

    if !normalizer.diagnostics.is_empty() {
        return Err(normalizer.diagnostics);
    }

    let entry = func_order
        .iter()
        .position(|(name, _)| name == "main")
        .unwrap() as FunctionId;

    let helpers: Vec<IrFunction> = normalizer
        .functions
        .iter()
        .enumerate()
        .filter(|(_, f)| f.name != "main")
        .map(|(i, f)| IrFunction {
            id: i as FunctionId,
            name: f.name.clone(),
            source_span: f.source_span,
            max_call_depth: f.helper_call_depth,
        })
        .collect();

    let max_helper_call_depth = helpers.iter().map(|h| h.max_call_depth).max().unwrap_or(0);

    let max_aggregate_results: u32 = normalizer
        .capability_calls
        .iter()
        .map(|c| c.max_results_per_call * c.max_calls)
        .sum();

    let capability_manifest = CapabilityManifest {
        capability_api_version: CAPABILITY_API_V1,
        calls: normalizer.capability_calls,
        required_input_fields: BTreeSet::new(),
        max_aggregate_results,
    };

    let static_cost = StaticCost {
        ast_nodes: facts.ast_nodes,
        literal_bytes: facts.literal_bytes,
        helper_count: helpers.len() as u32,
        max_helper_call_depth,
        max_capability_calls: capability_manifest.calls.len() as u32,
        max_aggregate_capability_results: max_aggregate_results,
        max_collection_traversals: normalizer.max_collection_traversals,
        max_output_candidates: normalizer.max_output_candidates,
        max_output_bytes: estimate_output_bytes(normalizer.max_output_candidates),
    };

    let mut limit_diagnostics = Vec::new();
    for (exceeded, message) in [
        (
            static_cost.max_capability_calls > limits.max_capability_calls,
            "capability call count exceeds the configured limit",
        ),
        (
            static_cost.max_collection_traversals > limits.max_collection_traversals,
            "collection traversal count exceeds the configured limit",
        ),
        (
            static_cost.max_output_candidates > limits.max_candidates,
            "output candidate count exceeds the configured limit",
        ),
        (
            static_cost.max_output_bytes > limits.max_output_bytes,
            "output byte bound exceeds the configured limit",
        ),
    ] {
        if exceeded {
            limit_diagnostics.push(SourceDiagnostic {
                code: DiagnosticCode::StaticLimitExceeded,
                severity: DiagnosticSeverity::Error,
                message: message.into(),
                primary_span: SourceSpan::from_offsets(source, 0, source.len() as u32),
                related: Vec::new(),
            });
        }
    }
    if !limit_diagnostics.is_empty() {
        return Err(limit_diagnostics);
    }

    Ok(WorkflowIr {
        ir_schema_version: IR_SCHEMA_V1,
        entry,
        helpers,
        nodes: normalizer.nodes,
        output: normalizer.output,
        capability_manifest,
        static_cost,
        possible_edit_kinds: normalizer.possible_edit_kinds,
    })
}

impl<'a> Normalizer<'a> {
    fn alloc_node(&mut self) -> NodeId {
        let id = self.next_node_id;
        self.next_node_id += 1;
        id
    }

    fn push_node(&mut self, node: IrNode) -> NodeId {
        let id = node.id;
        self.nodes.push(node);
        id
    }

    fn span_for_expr(&self, expr: &Expression<'_>) -> SourceSpan {
        SourceSpan::from_offsets(self.source, expr.span().start, expr.span().end)
    }

    fn normalize_statement(
        &mut self,
        stmt: &Statement<'a>,
        var_nodes: &mut BTreeMap<String, NodeId>,
    ) {
        match stmt {
            Statement::VariableDeclaration(decl) => {
                for declarator in &decl.declarations {
                    if let Some(init) = &declarator.init {
                        let (node_id, _cardinality) = self.normalize_expression(init, var_nodes);
                        if let BindingPattern::BindingIdentifier(id) = &declarator.id {
                            var_nodes.insert(id.name.to_string(), node_id);
                        }
                    }
                }
            }
            Statement::ReturnStatement(ret) => {
                if let Some(arg) = &ret.argument {
                    self.normalize_expression(arg, var_nodes);
                }
            }
            Statement::ExpressionStatement(expr_stmt) => {
                self.normalize_expression(&expr_stmt.expression, var_nodes);
            }
            _ => {}
        }
    }

    fn normalize_expression(
        &mut self,
        expr: &Expression<'a>,
        var_nodes: &BTreeMap<String, NodeId>,
    ) -> (NodeId, u32) {
        match expr {
            Expression::CallExpression(call) => self.normalize_call(call, var_nodes),
            Expression::ObjectExpression(obj) => self.normalize_object(obj, var_nodes),
            Expression::Identifier(ident) => {
                let (id, cardinality) = if let Some(&node_id) = var_nodes.get(ident.name.as_str()) {
                    let cardinality = self
                        .nodes
                        .iter()
                        .find(|node| node.id == node_id)
                        .map(|node| node.max_cardinality)
                        .unwrap_or(1);
                    (node_id, cardinality)
                } else {
                    let node_id = self.alloc_node();
                    self.push_node(IrNode {
                        id: node_id,
                        kind: IrNodeKind::Transform(TransformIr {
                            expression_summary: ident.name.to_string(),
                        }),
                        source_span: self.span_for_expr(expr),
                        max_cardinality: 1,
                    });
                    (node_id, 1)
                };
                (id, cardinality)
            }
            Expression::StringLiteral(_)
            | Expression::NumericLiteral(_)
            | Expression::BooleanLiteral(_)
            | Expression::NullLiteral(_) => {
                let summary = if let Expression::NumericLiteral(n) = expr {
                    format!("literal({})", n.value)
                } else {
                    "literal".into()
                };
                let node_id = self.alloc_node();
                self.push_node(IrNode {
                    id: node_id,
                    kind: IrNodeKind::Transform(TransformIr {
                        expression_summary: summary,
                    }),
                    source_span: self.span_for_expr(expr),
                    max_cardinality: 1,
                });
                (node_id, 1)
            }
            Expression::BinaryExpression(binary) => {
                let (left_id, _) = self.normalize_expression(&binary.left, var_nodes);
                let (right_id, _) = self.normalize_expression(&binary.right, var_nodes);
                let _ = (left_id, right_id);
                let node_id = self.alloc_node();
                self.push_node(IrNode {
                    id: node_id,
                    kind: IrNodeKind::Transform(TransformIr {
                        expression_summary: format!("binary({:?})", binary.operator),
                    }),
                    source_span: self.span_for_expr(expr),
                    max_cardinality: 1,
                });
                (node_id, 1)
            }
            Expression::UnaryExpression(unary) => {
                let (_, _) = self.normalize_expression(&unary.argument, var_nodes);
                let node_id = self.alloc_node();
                self.push_node(IrNode {
                    id: node_id,
                    kind: IrNodeKind::Transform(TransformIr {
                        expression_summary: "unary".into(),
                    }),
                    source_span: self.span_for_expr(expr),
                    max_cardinality: 1,
                });
                (node_id, 1)
            }
            Expression::LogicalExpression(logical) => {
                let (_, _) = self.normalize_expression(&logical.left, var_nodes);
                let (_, _) = self.normalize_expression(&logical.right, var_nodes);
                let node_id = self.alloc_node();
                self.push_node(IrNode {
                    id: node_id,
                    kind: IrNodeKind::Transform(TransformIr {
                        expression_summary: "logical".into(),
                    }),
                    source_span: self.span_for_expr(expr),
                    max_cardinality: 1,
                });
                (node_id, 1)
            }
            Expression::ConditionalExpression(cond) => {
                let (_, _) = self.normalize_expression(&cond.test, var_nodes);
                let (_, _) = self.normalize_expression(&cond.consequent, var_nodes);
                let (_, _) = self.normalize_expression(&cond.alternate, var_nodes);
                let node_id = self.alloc_node();
                self.push_node(IrNode {
                    id: node_id,
                    kind: IrNodeKind::Condition(ConditionIr {
                        expression_summary: "conditional".into(),
                    }),
                    source_span: self.span_for_expr(expr),
                    max_cardinality: 1,
                });
                (node_id, 1)
            }
            Expression::ParenthesizedExpression(paren) => {
                self.normalize_expression(&paren.expression, var_nodes)
            }
            Expression::ArrayExpression(arr) => {
                let card = arr.elements.len() as u32;
                for element in &arr.elements {
                    if let Some(e) = element.as_expression() {
                        let (_, _) = self.normalize_expression(e, var_nodes);
                    }
                }
                let node_id = self.alloc_node();
                self.push_node(IrNode {
                    id: node_id,
                    kind: IrNodeKind::Transform(TransformIr {
                        expression_summary: "array_literal".into(),
                    }),
                    source_span: self.span_for_expr(expr),
                    max_cardinality: card,
                });
                (node_id, card)
            }
            Expression::StaticMemberExpression(member) => {
                let (_, _) = self.normalize_expression(&member.object, var_nodes);
                let node_id = self.alloc_node();
                let is_input_derived =
                    matches!(&member.object, Expression::Identifier(id) if id.name == "input");
                let cardinality = if is_input_derived && member.property.name == "annotations" {
                    self.limits.max_input_annotations
                } else {
                    1
                };
                if is_input_derived && member.property.name != "annotations" {
                    self.unbounded_nodes.insert(node_id);
                }
                self.push_node(IrNode {
                    id: node_id,
                    kind: IrNodeKind::Transform(TransformIr {
                        expression_summary: "member_access".into(),
                    }),
                    source_span: self.span_for_expr(expr),
                    max_cardinality: cardinality,
                });
                (node_id, cardinality)
            }
            _ => {
                let node_id = self.alloc_node();
                self.push_node(IrNode {
                    id: node_id,
                    kind: IrNodeKind::Transform(TransformIr {
                        expression_summary: "other".into(),
                    }),
                    source_span: self.span_for_expr(expr),
                    max_cardinality: 1,
                });
                (node_id, 1)
            }
        }
    }

    fn normalize_call(
        &mut self,
        call: &CallExpression<'a>,
        var_nodes: &BTreeMap<String, NodeId>,
    ) -> (NodeId, u32) {
        match &call.callee {
            Expression::StaticMemberExpression(member) => {
                let method_name = member.property.name.as_str();

                match &member.object {
                    Expression::Identifier(obj) => {
                        let obj_name = obj.name.as_str();

                        if obj_name == "rollshot" {
                            self.normalize_rollshot_call(call, method_name)
                        } else if self.helper_names.contains(obj_name) {
                            self.normalize_helper_call(obj_name, call, var_nodes)
                        } else {
                            let (obj_id, _) = self.normalize_expression(&member.object, var_nodes);
                            if let Some(collection_kind) =
                                COLLECTION_METHODS.iter().find(|(m, _)| *m == method_name)
                            {
                                let (_, make_kind) = collection_kind;
                                self.normalize_collection_call(
                                    call,
                                    obj_id,
                                    make_kind(CollectionIr { input: 0 }),
                                    var_nodes,
                                )
                            } else {
                                self.normalize_generic_call(call, var_nodes)
                            }
                        }
                    }
                    Expression::CallExpression(inner_call) => {
                        let (inner_id, _) = self.normalize_call(inner_call, var_nodes);
                        if let Some(collection_kind) =
                            COLLECTION_METHODS.iter().find(|(m, _)| *m == method_name)
                        {
                            let (_, make_kind) = collection_kind;
                            self.normalize_collection_call(
                                call,
                                inner_id,
                                make_kind(CollectionIr { input: 0 }),
                                var_nodes,
                            )
                        } else {
                            self.normalize_generic_call_with_inner(call, inner_id, var_nodes)
                        }
                    }
                    _ => {
                        let (obj_id, _) = self.normalize_expression(&member.object, var_nodes);
                        if let Some(collection_kind) =
                            COLLECTION_METHODS.iter().find(|(m, _)| *m == method_name)
                        {
                            let (_, make_kind) = collection_kind;
                            self.normalize_collection_call(
                                call,
                                obj_id,
                                make_kind(CollectionIr { input: 0 }),
                                var_nodes,
                            )
                        } else {
                            self.normalize_generic_call(call, var_nodes)
                        }
                    }
                }
            }
            Expression::Identifier(ident) => {
                let name = ident.name.as_str();
                if self.helper_names.contains(name) {
                    self.normalize_helper_call(name, call, var_nodes)
                } else {
                    self.normalize_generic_call(call, var_nodes)
                }
            }
            _ => self.normalize_generic_call(call, var_nodes),
        }
    }

    fn normalize_rollshot_call(
        &mut self,
        call: &CallExpression<'a>,
        method_name: &str,
    ) -> (NodeId, u32) {
        let capability = ROLLSHOT_CAPABILITIES
            .iter()
            .find(|(name, _)| *name == method_name)
            .map(|(_, cap)| *cap);

        let Some(capability) = capability else {
            let node_id = self.alloc_node();
            self.push_node(IrNode {
                id: node_id,
                kind: IrNodeKind::Transform(TransformIr {
                    expression_summary: "unknown_capability".into(),
                }),
                source_span: SourceSpan::from_offsets(self.source, call.span.start, call.span.end),
                max_cardinality: 1,
            });
            return (node_id, 1);
        };

        let limit = self.extract_literal_limit(call).unwrap_or_else(|| {
            self.diagnostics.push(SourceDiagnostic {
                code: DiagnosticCode::StaticLimitExceeded,
                severity: DiagnosticSeverity::Error,
                message: "capability limit must be a positive bounded integer literal".into(),
                primary_span: SourceSpan::from_offsets(self.source, call.span.start, call.span.end),
                related: Vec::new(),
            });
            1
        });

        for arg in &call.arguments {
            if let Some(expr) = arg.as_expression() {
                let (_, _) = self.normalize_expression(expr, &BTreeMap::new());
            }
        }

        let node_id = self.alloc_node();
        self.push_node(IrNode {
            id: node_id,
            kind: IrNodeKind::CapabilityCall(CapabilityCallIr {
                capability,
                literal_limit: limit,
            }),
            source_span: SourceSpan::from_offsets(self.source, call.span.start, call.span.end),
            max_cardinality: limit,
        });

        self.capability_calls.push(CapabilityCallManifest {
            capability,
            source_span: SourceSpan::from_offsets(self.source, call.span.start, call.span.end),
            max_calls: 1,
            max_results_per_call: limit,
        });

        (node_id, limit)
    }

    fn normalize_helper_call(
        &mut self,
        helper_name: &str,
        call: &CallExpression<'a>,
        var_nodes: &BTreeMap<String, NodeId>,
    ) -> (NodeId, u32) {
        let mut argument_cardinalities = Vec::new();
        for arg in &call.arguments {
            if let Some(expr) = arg.as_expression() {
                let (_, cardinality) = self.normalize_expression(expr, var_nodes);
                argument_cardinalities.push(cardinality);
            }
        }
        let cardinality = self.infer_helper_cardinality(
            helper_name,
            &argument_cardinalities,
            &mut BTreeSet::new(),
        );

        let node_id = self.alloc_node();
        self.push_node(IrNode {
            id: node_id,
            kind: IrNodeKind::HelperCall(HelperCallIr {
                helper: helper_name.to_string(),
            }),
            source_span: SourceSpan::from_offsets(self.source, call.span.start, call.span.end),
            max_cardinality: cardinality,
        });

        (node_id, cardinality)
    }

    fn normalize_collection_call(
        &mut self,
        call: &CallExpression<'a>,
        input_node_id: NodeId,
        kind_template: IrNodeKind,
        var_nodes: &BTreeMap<String, NodeId>,
    ) -> (NodeId, u32) {
        if self.unbounded_nodes.contains(&input_node_id) {
            self.diagnostics.push(SourceDiagnostic {
                code: DiagnosticCode::UnboundedCollection,
                severity: DiagnosticSeverity::Error,
                message: "collection on input-derived value has no provable bound".into(),
                primary_span: SourceSpan::from_offsets(self.source, call.span.start, call.span.end),
                related: Vec::new(),
            });
            let node_id = self.alloc_node();
            return (node_id, 0);
        }

        self.max_collection_traversals += 1;

        let input_cardinality = self
            .nodes
            .iter()
            .find(|n| n.id == input_node_id)
            .map(|n| n.max_cardinality)
            .unwrap_or(1);

        for arg in &call.arguments {
            match arg {
                Argument::ArrowFunctionExpression(arrow) => {
                    self.normalize_arrow_body(arrow, var_nodes);
                }
                _ => {
                    if let Some(expr) = arg.as_expression() {
                        let (_, _) = self.normalize_expression(expr, var_nodes);
                    }
                }
            }
        }

        let mut kind = kind_template;
        match &mut kind {
            IrNodeKind::CollectionMap(ref mut c)
            | IrNodeKind::CollectionFilter(ref mut c)
            | IrNodeKind::CollectionSome(ref mut c)
            | IrNodeKind::CollectionEvery(ref mut c) => {
                c.input = input_node_id;
            }
            _ => {}
        }

        let node_id = self.alloc_node();
        self.push_node(IrNode {
            id: node_id,
            kind,
            source_span: SourceSpan::from_offsets(self.source, call.span.start, call.span.end),
            max_cardinality: input_cardinality,
        });

        (node_id, input_cardinality)
    }

    fn normalize_generic_call(
        &mut self,
        call: &CallExpression<'a>,
        var_nodes: &BTreeMap<String, NodeId>,
    ) -> (NodeId, u32) {
        for arg in &call.arguments {
            if let Some(expr) = arg.as_expression() {
                let (_, _) = self.normalize_expression(expr, var_nodes);
            }
        }

        let node_id = self.alloc_node();
        self.push_node(IrNode {
            id: node_id,
            kind: IrNodeKind::Transform(TransformIr {
                expression_summary: "call".into(),
            }),
            source_span: SourceSpan::from_offsets(self.source, call.span.start, call.span.end),
            max_cardinality: 1,
        });

        (node_id, 1)
    }

    fn normalize_generic_call_with_inner(
        &mut self,
        call: &CallExpression<'a>,
        _inner_id: NodeId,
        var_nodes: &BTreeMap<String, NodeId>,
    ) -> (NodeId, u32) {
        self.normalize_generic_call(call, var_nodes)
    }

    fn normalize_arrow_body(
        &mut self,
        arrow: &ArrowFunctionExpression<'a>,
        var_nodes: &BTreeMap<String, NodeId>,
    ) {
        let mut arrow_vars = var_nodes.clone();
        for param in &arrow.params.items {
            if let BindingPattern::BindingIdentifier(id) = &param.pattern {
                arrow_vars.insert(id.name.to_string(), 0);
            }
        }
        for stmt in &arrow.body.statements {
            self.normalize_statement(stmt, &mut arrow_vars);
        }
    }

    fn normalize_object(
        &mut self,
        obj: &ast::ObjectExpression<'a>,
        var_nodes: &BTreeMap<String, NodeId>,
    ) -> (NodeId, u32) {
        let mut emit_input = None;

        for prop_kind in &obj.properties {
            if let ObjectPropertyKind::ObjectProperty(prop) = prop_kind {
                if let PropertyKey::StaticIdentifier(key) = &prop.key {
                    let key_str = key.name.as_str();
                    let (val_id, _card) = self.normalize_expression(&prop.value, var_nodes);

                    if key_str == "candidates" {
                        if let Some(input_node) = self.find_emit_input(&prop.value, var_nodes) {
                            emit_input = Some(input_node);
                        } else {
                            emit_input = Some(val_id);
                        }
                    } else if key_str == "kind" {
                        if let Expression::StringLiteral(lit) = &prop.value {
                            let kind_str = lit.value.as_str();
                            if let Some(kind) = parse_edit_kind(kind_str) {
                                self.possible_edit_kinds.insert(kind);
                            }
                        }
                    }
                }
            }
        }

        if let Some(input_id) = emit_input {
            let input_card = self
                .nodes
                .iter()
                .find(|n| n.id == input_id)
                .map(|n| n.max_cardinality)
                .unwrap_or(1);

            let node_id = self.alloc_node();
            self.push_node(IrNode {
                id: node_id,
                kind: IrNodeKind::EmitCandidates(EmitCandidatesIr { input: input_id }),
                source_span: SourceSpan::from_offsets(self.source, obj.span.start, obj.span.end),
                max_cardinality: input_card,
            });
            (node_id, input_card)
        } else {
            let node_id = self.alloc_node();
            self.push_node(IrNode {
                id: node_id,
                kind: IrNodeKind::Transform(TransformIr {
                    expression_summary: "object_literal".into(),
                }),
                source_span: SourceSpan::from_offsets(self.source, obj.span.start, obj.span.end),
                max_cardinality: 1,
            });
            (node_id, 1)
        }
    }

    fn find_emit_input(
        &self,
        expr: &Expression<'a>,
        var_nodes: &BTreeMap<String, NodeId>,
    ) -> Option<NodeId> {
        match expr {
            Expression::CallExpression(call) => {
                if let Expression::StaticMemberExpression(member) = &call.callee {
                    let method_name = member.property.name.as_str();
                    if COLLECTION_METHODS.iter().any(|(m, _)| *m == method_name) {
                        match &member.object {
                            Expression::Identifier(ident) => {
                                return var_nodes.get(ident.name.as_str()).copied();
                            }
                            Expression::CallExpression(inner) => {
                                return self.find_call_node_id(inner, var_nodes);
                            }
                            _ => {}
                        }
                    }
                }
                None
            }
            Expression::Identifier(ident) => var_nodes.get(ident.name.as_str()).copied(),
            _ => None,
        }
    }

    fn find_call_node_id(
        &self,
        call: &CallExpression<'a>,
        var_nodes: &BTreeMap<String, NodeId>,
    ) -> Option<NodeId> {
        match &call.callee {
            Expression::StaticMemberExpression(member) => match &member.object {
                Expression::Identifier(obj) => {
                    if obj.name == "rollshot" {
                        let cap = ROLLSHOT_CAPABILITIES
                            .iter()
                            .find(|(name, _)| *name == member.property.name.as_str())
                            .map(|(_, cap)| *cap)?;
                        self.nodes
                            .iter()
                            .find(|n| {
                                matches!(&n.kind, IrNodeKind::CapabilityCall(c) if
                                        c.capability == cap)
                            })
                            .map(|n| n.id)
                    } else {
                        var_nodes.get(obj.name.as_str()).copied()
                    }
                }
                Expression::CallExpression(inner) => self.find_call_node_id(inner, var_nodes),
                _ => None,
            },
            Expression::Identifier(ident) => var_nodes.get(ident.name.as_str()).copied(),
            _ => None,
        }
    }

    fn extract_literal_limit(&self, call: &CallExpression<'a>) -> Option<u32> {
        for arg in &call.arguments {
            if let Some(Expression::ObjectExpression(obj)) = arg.as_expression() {
                for prop_kind in &obj.properties {
                    if let ObjectPropertyKind::ObjectProperty(prop) = prop_kind {
                        if let PropertyKey::StaticIdentifier(key) = &prop.key {
                            if key.name == "limit" {
                                if let Expression::NumericLiteral(lit) = &prop.value {
                                    if lit.value.is_finite()
                                        && lit.value.fract() == 0.0
                                        && lit.value > 0.0
                                        && lit.value <= MAX_CAPABILITY_RESULTS_PER_CALL
                                    {
                                        return Some(lit.value as u32);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        None
    }

    fn infer_helper_cardinality(
        &self,
        helper_name: &str,
        argument_cardinalities: &[u32],
        visiting: &mut BTreeSet<String>,
    ) -> u32 {
        if !visiting.insert(helper_name.to_string()) {
            return 0;
        }
        let Some(function) = self.helper_functions.get(helper_name) else {
            visiting.remove(helper_name);
            return 1;
        };
        let mut variables = BTreeMap::new();
        for (parameter, cardinality) in function
            .params
            .items
            .iter()
            .zip(argument_cardinalities.iter().copied())
        {
            if let BindingPattern::BindingIdentifier(identifier) = &parameter.pattern {
                variables.insert(identifier.name.to_string(), cardinality);
            }
        }
        let mut result = 1;
        if let Some(body) = &function.body {
            for statement in &body.statements {
                match statement {
                    Statement::VariableDeclaration(declaration) => {
                        for declarator in &declaration.declarations {
                            if let (BindingPattern::BindingIdentifier(identifier), Some(value)) =
                                (&declarator.id, &declarator.init)
                            {
                                let cardinality =
                                    self.infer_expression_cardinality(value, &variables, visiting);
                                variables.insert(identifier.name.to_string(), cardinality);
                            }
                        }
                    }
                    Statement::ReturnStatement(return_statement) => {
                        if let Some(value) = &return_statement.argument {
                            result = self.infer_expression_cardinality(value, &variables, visiting);
                        }
                    }
                    _ => {}
                }
            }
        }
        visiting.remove(helper_name);
        result
    }

    fn infer_expression_cardinality(
        &self,
        expression: &Expression<'a>,
        variables: &BTreeMap<String, u32>,
        visiting: &mut BTreeSet<String>,
    ) -> u32 {
        match expression {
            Expression::Identifier(identifier) => variables
                .get(identifier.name.as_str())
                .copied()
                .unwrap_or(1),
            Expression::ArrayExpression(array) => array.elements.len() as u32,
            Expression::ParenthesizedExpression(parenthesized) => {
                self.infer_expression_cardinality(&parenthesized.expression, variables, visiting)
            }
            Expression::ConditionalExpression(conditional) => self
                .infer_expression_cardinality(&conditional.consequent, variables, visiting)
                .max(self.infer_expression_cardinality(
                    &conditional.alternate,
                    variables,
                    visiting,
                )),
            Expression::StaticMemberExpression(member) => {
                if matches!(&member.object, Expression::Identifier(id) if id.name == "input")
                    && member.property.name == "annotations"
                {
                    self.limits.max_input_annotations
                } else {
                    1
                }
            }
            Expression::CallExpression(call) => match &call.callee {
                Expression::Identifier(identifier)
                    if self.helper_functions.contains_key(identifier.name.as_str()) =>
                {
                    let argument_cardinalities = call
                        .arguments
                        .iter()
                        .filter_map(Argument::as_expression)
                        .map(|argument| {
                            self.infer_expression_cardinality(argument, variables, visiting)
                        })
                        .collect::<Vec<_>>();
                    self.infer_helper_cardinality(
                        identifier.name.as_str(),
                        &argument_cardinalities,
                        visiting,
                    )
                }
                Expression::StaticMemberExpression(member)
                    if COLLECTION_METHODS
                        .iter()
                        .any(|(method, _)| *method == member.property.name.as_str()) =>
                {
                    if matches!(member.property.name.as_str(), "some" | "every") {
                        1
                    } else {
                        self.infer_expression_cardinality(&member.object, variables, visiting)
                    }
                }
                _ => 1,
            },
            _ => 1,
        }
    }
}

fn estimate_output_bytes(max_candidates: u32) -> usize {
    let candidate_bytes = crate::output::MAX_LABEL_BYTES
        .saturating_add(crate::output::MAX_RATIONALE_BYTES)
        .saturating_add(crate::output::MAX_TEXT_BYTES)
        .saturating_add(crate::output::MAX_CANDIDATE_STRUCTURAL_BYTES);
    32_usize.saturating_add((max_candidates as usize).saturating_mul(candidate_bytes))
}

fn compute_max_call_depth(functions: &[super::validate::FunctionFacts]) -> u32 {
    functions
        .iter()
        .filter(|f| f.name != "main")
        .map(|f| compute_depth_for(&f.name, functions, &mut BTreeSet::new()))
        .max()
        .unwrap_or(0)
}

fn compute_depth_for(
    name: &str,
    functions: &[super::validate::FunctionFacts],
    visiting: &mut BTreeSet<String>,
) -> u32 {
    if visiting.contains(name) {
        return 0;
    }
    visiting.insert(name.to_string());
    let Some(facts) = functions.iter().find(|f| f.name == name) else {
        return 0;
    };
    let max_child = facts
        .calls
        .iter()
        .map(|(callee, _)| compute_depth_for(callee, functions, visiting))
        .max()
        .unwrap_or(0);
    visiting.remove(name);
    max_child + 1
}

fn parse_edit_kind(s: &str) -> Option<ProposedEditKind> {
    match s {
        "addRedaction" => Some(ProposedEditKind::AddRedaction),
        "addTextNote" => Some(ProposedEditKind::AddTextNote),
        "addNumberCallout" => Some(ProposedEditKind::AddNumberCallout),
        "updateRedactionBounds" => Some(ProposedEditKind::UpdateRedactionBounds),
        "updateTextPosition" => Some(ProposedEditKind::UpdateTextPosition),
        "updateText" => Some(ProposedEditKind::UpdateText),
        "updateNumberPoints" => Some(ProposedEditKind::UpdateNumberPoints),
        "delete" => Some(ProposedEditKind::Delete),
        _ => None,
    }
}

fn missing_main_diagnostic(source: &str, end: u32) -> SourceDiagnostic {
    SourceDiagnostic {
        code: DiagnosticCode::MissingMain,
        severity: DiagnosticSeverity::Error,
        message: "define exactly one synchronous function main(input)".into(),
        primary_span: SourceSpan::from_offsets(source, 0, end),
        related: Vec::new(),
    }
}
