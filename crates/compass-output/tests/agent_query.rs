use std::error::Error;

use compass_model::code_graph::{EdgeKind, NodeKind};
use compass_model::provenance::{
    EvidenceConfidence, EvidenceOrigin, ResolutionState, SourceAnchor,
};
use compass_model::query_contract::{
    CodeQueryLimits, CodeQueryOperation, CodeQueryResponse, QueryDiagnostic, QueryDiagnosticCode,
    QueryEdge, QueryEvidence, QueryEvidenceLayer, QueryNode, QueryPath, SearchHit,
};
use compass_output::{
    AGENT_BRIEF_VIEW_SCHEMA, AgentEvidence, AgentExecution, AgentMatch, AgentOperation,
    AgentQueryContext, AgentResultState, AgentTextPageOptions, build_code_query_brief,
    build_code_query_view, render_agent_query_text, render_code_query_text_page,
};

fn anchor(file: &str, line: u32) -> SourceAnchor {
    SourceAnchor {
        file: file.to_owned(),
        start_byte: 0,
        end_byte: 4,
        start_line: line,
        start_column: 0,
        end_line: line,
        end_column: 4,
    }
}

fn evidence(source: &SourceAnchor) -> QueryEvidence {
    QueryEvidence {
        layer: QueryEvidenceLayer::StructuralGraph,
        origin: EvidenceOrigin::Ast,
        extractor: "agent-query-test".to_owned(),
        confidence: EvidenceConfidence::Exact,
        anchor: Some(source.clone()),
        rule: None,
        wiring_site: None,
        resolution: ResolutionState::Exact,
        candidates: Vec::new(),
    }
}

fn node(id: &str, label: &str, source: &SourceAnchor) -> QueryNode {
    QueryNode {
        id: id.to_owned(),
        kind: NodeKind::Function,
        roles: Vec::new(),
        name: label.to_owned(),
        qualified_name: format!("Fixture.{label}"),
        language: Some("rust".to_owned()),
        framework: None,
        source: Some(source.clone()),
        details: None,
        evidence: vec![evidence(source)],
    }
}

fn response(operation: CodeQueryOperation) -> CodeQueryResponse {
    CodeQueryResponse::empty(operation, CodeQueryLimits::default())
}

fn context(operation: AgentOperation) -> AgentQueryContext {
    AgentQueryContext::new(operation, "graph-identity", "generation-identity")
}

#[test]
fn brief_projection_keeps_answer_semantics_and_drops_audit_detail() -> Result<(), Box<dyn Error>> {
    let caller_anchor = anchor("src/caller.rs", 10);
    let target_anchor = anchor("src/target.rs", 20);
    let mut response = response(CodeQueryOperation::Callers);
    response.nodes = vec![
        node("n:caller", "Caller", &caller_anchor),
        node("n:target", "Target", &target_anchor),
    ];
    response.edges.push(QueryEdge {
        id: "e:caller-target".to_owned(),
        source: "n:caller".to_owned(),
        target: "n:target".to_owned(),
        kind: EdgeKind::Calls,
        relationship_site: Some(caller_anchor.clone()),
        details: None,
        evidence: vec![evidence(&caller_anchor)],
    });
    let mut query_context = context(AgentOperation::Callers);
    query_context = query_context.with_operand(compass_output::AgentOperandRole::Symbol, "Target");

    let brief = build_code_query_brief(&response, query_context.clone())?;
    assert_eq!(brief.schema, AGENT_BRIEF_VIEW_SCHEMA);
    assert_eq!(brief.status.result_state, "answered");
    assert_eq!(brief.relationships.len(), 1);
    assert!(brief.relationships[0].source.contains("Caller"));
    assert_eq!(brief.relationships[0].relation, "calls");
    assert!(brief.relationships[0].target.contains("Target"));
    assert!(
        brief.relationships[0]
            .site
            .as_deref()
            .is_some_and(|site| site.starts_with("src/caller.rs"))
    );

    let full = build_code_query_view(&response, query_context)?;
    assert_eq!(
        brief.relationships[0].source,
        full.relationships[0].source.label
    );
    let brief_bytes = serde_json::to_vec(&brief)?.len();
    let full_bytes = serde_json::to_vec(&full)?.len();
    assert!(
        brief_bytes < full_bytes,
        "brief {brief_bytes} bytes must be smaller than {full_bytes}"
    );
    let brief_json = serde_json::to_string(&brief)?;
    assert!(!brief_json.contains("\"identity\""));
    assert!(!brief_json.contains("viewDigest"));
    // The status, headline, and caveats stay readable.
    assert!(brief_json.contains("\"caveats\""));
    assert_eq!(brief.answer, full.answer.headline);
    Ok(())
}

#[test]
fn paged_text_covers_records_beyond_the_compact_view_and_continues() -> Result<(), Box<dyn Error>> {
    let caller_anchor = anchor("src/caller.rs", 10);
    let target_anchor = anchor("src/target.rs", 20);
    let mut response = response(CodeQueryOperation::Callers);
    response
        .nodes
        .push(node("n:target", "Target", &target_anchor));
    for index in 0..40 {
        let id = format!("n:caller-{index:02}");
        response
            .nodes
            .push(node(&id, &format!("Caller{index:02}"), &caller_anchor));
        response.edges.push(QueryEdge {
            id: format!("e:caller-{index:02}"),
            source: id,
            target: "n:target".to_owned(),
            kind: EdgeKind::Calls,
            relationship_site: Some(caller_anchor.clone()),
            details: None,
            evidence: vec![evidence(&caller_anchor)],
        });
    }
    let mut query_context = context(AgentOperation::Callers);
    query_context = query_context.with_operand(compass_output::AgentOperandRole::Symbol, "Target");

    let first = render_code_query_text_page(
        &response,
        query_context.clone(),
        AgentTextPageOptions {
            token_budget: 300,
            cursor: None,
        },
    )?;
    assert!(first.entry_total > 24, "ledger must exceed the view bound");
    assert!(first.entry_end > first.entry_start);
    assert!(first.text.contains("Pagination:"));
    let cursor = first.next_cursor.clone().ok_or("expected a continuation")?;

    let second = render_code_query_text_page(
        &response,
        query_context.clone(),
        AgentTextPageOptions {
            token_budget: 300,
            cursor: Some(&cursor),
        },
    )?;
    assert_eq!(second.entry_start, first.entry_end);
    assert!(second.entry_end > second.entry_start);
    assert!(second.page > first.page);

    let tampered = format!("{}x", &cursor[..cursor.len() - 1]);
    assert!(
        render_code_query_text_page(
            &response,
            query_context,
            AgentTextPageOptions {
                token_budget: 300,
                cursor: Some(&tampered),
            },
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn paged_text_reaches_the_end_without_a_continuation() -> Result<(), Box<dyn Error>> {
    let caller_anchor = anchor("src/caller.rs", 10);
    let target_anchor = anchor("src/target.rs", 20);
    let mut response = response(CodeQueryOperation::Callers);
    response.nodes = vec![
        node("n:caller", "Caller", &caller_anchor),
        node("n:target", "Target", &target_anchor),
    ];
    response.edges.push(QueryEdge {
        id: "e:caller-target".to_owned(),
        source: "n:caller".to_owned(),
        target: "n:target".to_owned(),
        kind: EdgeKind::Calls,
        relationship_site: Some(caller_anchor.clone()),
        details: None,
        evidence: vec![evidence(&caller_anchor)],
    });
    let mut query_context = context(AgentOperation::Callers);
    query_context = query_context.with_operand(compass_output::AgentOperandRole::Symbol, "Target");
    let mut cursor: Option<String> = None;
    let mut pages = 0_u32;
    loop {
        let page = render_code_query_text_page(
            &response,
            query_context.clone(),
            AgentTextPageOptions {
                token_budget: 2_000,
                cursor: cursor.as_deref(),
            },
        )?;
        pages += 1;
        match page.next_cursor {
            Some(next) => cursor = Some(next),
            None => {
                assert_eq!(page.entry_end, page.entry_total);
                break;
            }
        }
        assert!(pages < 10, "pagination must terminate");
    }
    Ok(())
}

#[test]
fn exact_relationship_view_is_answer_first_and_round_trips() -> Result<(), Box<dyn Error>> {
    let caller_anchor = anchor("src/caller.rs", 10);
    let target_anchor = anchor("src/target.rs", 20);
    let mut response = response(CodeQueryOperation::Callers);
    response.nodes = vec![
        node("n:caller", "Caller", &caller_anchor),
        node("n:target", "Target", &target_anchor),
    ];
    response.edges.push(QueryEdge {
        id: "e:caller-target".to_owned(),
        source: "n:caller".to_owned(),
        target: "n:target".to_owned(),
        kind: EdgeKind::Calls,
        relationship_site: Some(caller_anchor),
        details: None,
        evidence: vec![evidence(&target_anchor)],
    });
    let view = build_code_query_view(
        &response,
        context(AgentOperation::Callers)
            .with_operand(compass_output::AgentOperandRole::Symbol, "Target"),
    )?;
    assert_eq!(view.status.result_state, AgentResultState::Answered);
    assert_eq!(view.status.match_state, AgentMatch::Exact);
    assert_eq!(view.status.evidence_state, AgentEvidence::Exact);
    assert_eq!(view.status.source_execution, AgentExecution::Complete);
    assert_eq!(view.relationships[0].source.label, "Fixture.Caller");
    assert_eq!(view.relationships[0].target.label, "Fixture.Target");
    let text = render_agent_query_text(&view)?;
    assert!(text.starts_with("RESULT\n"));
    assert!(
        text.find("ANSWER").ok_or("missing answer")?
            < text
                .find("PRIMARY RESULTS")
                .ok_or("missing primary results")?
    );
    assert!(text.contains("Fixture.Caller --calls--> Fixture.Target"));
    let json = serde_json::to_vec(&view)?;
    assert_eq!(compass_output::AgentQueryView::from_json(&json)?, view);
    Ok(())
}

#[test]
fn no_match_and_ambiguous_results_are_not_positive_answers() -> Result<(), Box<dyn Error>> {
    let mut response = response(CodeQueryOperation::Search);
    response.diagnostics.push(QueryDiagnostic {
        code: QueryDiagnosticCode::NoMatch,
        message: "NO EXACT MATCH for Targat".to_owned(),
        node_id: None,
        path: None,
    });
    let view = build_code_query_view(
        &response,
        context(AgentOperation::Search)
            .with_operand(compass_output::AgentOperandRole::Query, "Targat"),
    )?;
    assert_eq!(view.status.result_state, AgentResultState::NoMatch);
    assert_eq!(view.status.match_state, AgentMatch::None);
    assert!(view.answer.headline.starts_with("No exact match"));
    assert!(view.caveats.iter().any(|caveat| caveat.code == "no_match"));
    let text = render_agent_query_text(&view)?;
    assert!(
        text.find("CAVEATS").ok_or("missing caveats")?
            < text
                .find("PRIMARY RESULTS")
                .ok_or("missing primary results")?
    );
    Ok(())
}

#[test]
fn unresolved_ambiguity_without_retained_candidates_stays_explicit() -> Result<(), Box<dyn Error>> {
    let mut response = response(CodeQueryOperation::Callers);
    response.diagnostics.push(QueryDiagnostic {
        code: QueryDiagnosticCode::AmbiguousMatch,
        message: "Symbol Target matched multiple nodes".to_owned(),
        node_id: None,
        path: None,
    });
    let view = build_code_query_view(
        &response,
        context(AgentOperation::Callers)
            .with_operand(compass_output::AgentOperandRole::Symbol, "Target"),
    )?;
    assert_eq!(view.status.result_state, AgentResultState::NeedsResolution);
    assert!(view.primary_results.is_empty());
    assert!(
        view.answer
            .basis
            .iter()
            .any(|basis| basis.kind == "operation")
    );
    Ok(())
}

#[test]
fn reverse_path_keeps_published_direction_visible() -> Result<(), Box<dyn Error>> {
    let source_anchor = anchor("src/source.rs", 1);
    let target_anchor = anchor("src/target.rs", 2);
    let mut response = response(CodeQueryOperation::NodeTrail);
    response.nodes = vec![
        node("n:source", "Source", &source_anchor),
        node("n:target", "Target", &target_anchor),
    ];
    response.edges.push(QueryEdge {
        id: "e:source-target".to_owned(),
        source: "n:source".to_owned(),
        target: "n:target".to_owned(),
        kind: EdgeKind::Calls,
        relationship_site: Some(source_anchor.clone()),
        details: None,
        evidence: vec![evidence(&source_anchor)],
    });
    response.paths.push(QueryPath {
        id: "p:reverse".to_owned(),
        node_ids: vec!["n:target".to_owned(), "n:source".to_owned()],
        edge_ids: vec!["e:source-target".to_owned()],
        weakest_resolution: ResolutionState::Exact,
        weakest_confidence: EvidenceConfidence::Exact,
    });
    let view = build_code_query_view(
        &response,
        context(AgentOperation::NodeTrail)
            .with_operand(compass_output::AgentOperandRole::Source, "Source")
            .with_operand(compass_output::AgentOperandRole::Target, "Target"),
    )?;
    assert_eq!(view.status.result_state, AgentResultState::Answered);
    assert_eq!(
        view.paths[0].steps[0].direction,
        compass_output::AgentPathDirection::Reverse
    );
    Ok(())
}

#[test]
fn direction_mismatch_is_a_no_path_blocker() -> Result<(), Box<dyn Error>> {
    let source_anchor = anchor("src/source.rs", 1);
    let target_anchor = anchor("src/target.rs", 2);
    let mut response = response(CodeQueryOperation::NodeTrail);
    response.nodes = vec![
        node("n:source", "Source", &source_anchor),
        node("n:target", "Target", &target_anchor),
    ];
    response.diagnostics.push(QueryDiagnostic {
        code: QueryDiagnosticCode::DirectionMismatch,
        message: "A reverse-only connection was found".to_owned(),
        node_id: Some("n:source".to_owned()),
        path: None,
    });
    let view = build_code_query_view(
        &response,
        context(AgentOperation::NodeTrail)
            .with_operand(compass_output::AgentOperandRole::Source, "Source")
            .with_operand(compass_output::AgentOperandRole::Target, "Target"),
    )?;
    assert_eq!(view.status.result_state, AgentResultState::NoPath);
    assert!(
        view.caveats
            .iter()
            .any(|caveat| caveat.code == "direction_mismatch")
    );
    assert!(view.next_actions.iter().any(|action| {
        action.kind == "inspect_undirected_path"
            && action
                .cli
                .as_ref()
                .is_some_and(|cli| cli.argv == ["compass", "path", "Source", "Target"])
    }));
    Ok(())
}

#[test]
fn equivalent_collection_order_has_one_view_digest() -> Result<(), Box<dyn Error>> {
    let first_anchor = anchor("src/first.rs", 1);
    let second_anchor = anchor("src/second.rs", 2);
    let mut left = response(CodeQueryOperation::Search);
    left.nodes = vec![
        node("n:first", "First", &first_anchor),
        node("n:second", "Second", &second_anchor),
    ];
    left.results.push(SearchHit {
        node_id: "n:first".to_owned(),
        score: 1.0,
        matched_fields: vec!["name".to_owned()],
    });
    let mut right = left.clone();
    right.nodes.reverse();
    let left_view = build_code_query_view(
        &left,
        context(AgentOperation::Search)
            .with_operand(compass_output::AgentOperandRole::Query, "First"),
    )?;
    let right_view = build_code_query_view(
        &right,
        context(AgentOperation::Search)
            .with_operand(compass_output::AgentOperandRole::Query, "First"),
    )?;
    assert_eq!(
        left_view.identity.source_result_digest,
        right_view.identity.source_result_digest
    );
    assert_eq!(
        left_view.identity.view_digest,
        right_view.identity.view_digest
    );
    Ok(())
}
