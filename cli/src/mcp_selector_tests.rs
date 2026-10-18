use super::*;

#[test]
fn mcp_keeps_selector_errors_distinct_from_matched_empty_text() {
    for (exit_code, response, is_error) in [
        (1, json!({"success":false,"data":null,"error":"Selector evaluation failed for [: SyntaxError: invalid selector"}), true),
        (0, json!({"success":true,"data":{"text":"","origin":"data:text/html,fixture"}}), false),
    ] {
        let run = CliRun { exit_code:Some(exit_code), stdout:response.to_string(), stderr:String::new() };
        let result = tool_result_from_run(run);
        assert_eq!(result["isError"], is_error);
        assert_eq!(result["structuredContent"]["response"], response);
    }
}
