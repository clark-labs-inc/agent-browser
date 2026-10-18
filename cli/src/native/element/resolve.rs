use super::*;

/// Find a selector inside a same-process iframe and return its object handle.
async fn resolve_object_in_same_process_frame(
    client: &CdpClient,
    session_id: &str,
    frame_id: &str,
    selector: &str,
) -> Result<String, String> {
    let owner_object_id = frame_owner_object_id(client, session_id, frame_id).await?;
    let find_expr = build_find_element_js_in("doc", selector);
    let function = format!(
        "function() {{ const doc = this.contentDocument; if (!doc) return null; return {find_expr}; }}",
    );
    let result: EvaluateResult = client
        .send_command_typed(
            "Runtime.callFunctionOn",
            &serde_json::json!({
                "objectId": owner_object_id,
                "functionDeclaration": function,
                "returnByValue": false,
            }),
            Some(session_id),
        )
        .await?;
    checked_element_object_id(result, selector)
}

pub async fn resolve_element_object_id(
    client: &CdpClient,
    session_id: &str,
    ref_map: &RefMap,
    selector_or_ref: &str,
    iframe_sessions: &HashMap<String, String>,
) -> Result<(String, String), String> {
    if let Some(ref_id) = parse_ref(selector_or_ref) {
        let entry = ref_map
            .get(&ref_id)
            .ok_or_else(|| format!("Unknown ref: {}", ref_id))?;

        let effective_session_id =
            resolve_frame_session(entry.frame_id.as_deref(), session_id, iframe_sessions);

        // Try cached backend_node_id first (fast path)
        if let Some(backend_node_id) = entry.backend_node_id {
            let result: Result<DomResolveNodeResult, String> = client
                .send_command_typed(
                    "DOM.resolveNode",
                    &DomResolveNodeParams {
                        backend_node_id: Some(backend_node_id),
                        node_id: None,
                        object_group: Some("agent-browser".to_string()),
                    },
                    Some(effective_session_id),
                )
                .await;

            if let Ok(r) = result {
                if let Some(object_id) = r.object.object_id {
                    return Ok((object_id, effective_session_id.to_string()));
                }
            }
            // backend_node_id is stale; re-query the accessibility tree below
        }

        // Fallback: re-query the accessibility tree to find a fresh node by role/name
        let fresh_id = find_node_id_by_role_name(
            client,
            session_id,
            &entry.role,
            &entry.name,
            entry.nth,
            entry.frame_id.as_deref(),
            iframe_sessions,
        )
        .await?;
        let result: DomResolveNodeResult = client
            .send_command_typed(
                "DOM.resolveNode",
                &DomResolveNodeParams {
                    backend_node_id: Some(fresh_id),
                    node_id: None,
                    object_group: Some("agent-browser".to_string()),
                },
                Some(effective_session_id),
            )
            .await?;
        let object_id = result
            .object
            .object_id
            .ok_or_else(|| format!("No objectId for ref {}", ref_id))?;
        return Ok((object_id, effective_session_id.to_string()));
    }

    // Selector fallback (CSS or XPath): honor an active `frame <sel>` selection.
    if let Some(frame_id) = active_frame() {
        if let Some(frame_session) = iframe_sessions.get(&frame_id) {
            let js = build_find_element_js(selector_or_ref);
            let result: EvaluateResult = client
                .send_command_typed(
                    "Runtime.evaluate",
                    &EvaluateParams {
                        expression: js,
                        return_by_value: Some(false),
                        await_promise: Some(false),
                    },
                    Some(frame_session.as_str()),
                )
                .await?;
            let object_id = checked_element_object_id(result, selector_or_ref)?;
            return Ok((object_id, frame_session.clone()));
        }
        let object_id =
            resolve_object_in_same_process_frame(client, session_id, &frame_id, selector_or_ref)
                .await?;
        return Ok((object_id, session_id.to_string()));
    }

    let js = build_find_element_js(selector_or_ref);
    let result: EvaluateResult = client
        .send_command_typed(
            "Runtime.evaluate",
            &EvaluateParams {
                expression: js,
                return_by_value: Some(false),
                await_promise: Some(false),
            },
            Some(session_id),
        )
        .await?;

    let object_id = checked_element_object_id(result, selector_or_ref)?;
    Ok((object_id, session_id.to_string()))
}


/// A thrown DOM exception also has an objectId. Only use the evaluation's
/// object handle after checking its exception receipt, otherwise getters read
/// the exception object and turn a failed selector into successful empty data.
fn checked_element_object_id(result: EvaluateResult, selector: &str) -> Result<String, String> {
    if let Some(details) = result.exception_details {
        let message = details.exception.and_then(|exception| exception.description)
            .unwrap_or(details.text);
        return Err(format!("Selector evaluation failed for {selector}: {message}"));
    }
    if result.result.subtype.as_deref() == Some("error") {
        let message = result.result.description.as_deref().unwrap_or("JavaScript error");
        return Err(format!("Selector evaluation failed for {selector}: {message}"));
    }
    result.result.object_id.ok_or_else(|| format!("Element not found: {selector}"))
}

#[cfg(test)]
#[path = "resolve_tests.rs"]
mod tests;
