use super::*;
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_tungstenite::{accept_async, tungstenite::Message};

fn selector_exception() -> Value {
    json!({
        "result":{"type":"object","subtype":"error","className":"DOMException",
            "description":"SyntaxError: invalid selector", "objectId":"exception-object"},
        "exceptionDetails":{"text":"Uncaught", "exception":{
            "type":"object","subtype":"error","description":"SyntaxError: invalid selector"}}
    })
}

async fn mock_client(evaluation: Value) -> (CdpClient, JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let mut ws = accept_async(socket).await.unwrap();
        let mut methods = Vec::new();
        while let Ok(Some(Ok(Message::Text(text)))) = tokio::time::timeout(
            std::time::Duration::from_millis(100), ws.next(),
        ).await {
            let request: Value = serde_json::from_str(&text).unwrap();
            let method = request["method"].as_str().unwrap().to_string();
            let result = match method.as_str() {
                "Runtime.evaluate" => evaluation.clone(),
                "Runtime.callFunctionOn" => {
                    if request["params"]["functionDeclaration"].as_str().unwrap_or("").contains("contentDocument") {
                        evaluation.clone()
                    } else { json!({"result":{"type":"string","value":""}}) }
                },
                "DOM.getFrameOwner" => json!({"backendNodeId":1}),
                "DOM.resolveNode" => json!({"object":{"type":"object","subtype":"node","objectId":"frame-owner"}}),
                _ => panic!("Unexpected CDP command: {method}"),
            };
            methods.push(method);
            ws.send(Message::Text(json!({"id":request["id"],"result":result}).to_string())).await.unwrap();
        }
        methods
    });
    let client = CdpClient::connect(&format!("ws://{address}")).await.unwrap();
    (client, server)
}

#[tokio::test]
async fn selector_exception_object_is_not_read_as_a_dom_element() {
    let (client, server) = mock_client(selector_exception()).await;
    let result = super::super::get_element_text(&client, "page", &RefMap::new(), "[", &HashMap::new()).await;
    let methods = server.await.unwrap();
    let error = result.expect_err("A selector syntax exception must not become successful empty text");
    assert!(error.contains("SyntaxError: invalid selector"), "{error}");
    assert_eq!(methods, ["Runtime.evaluate"]);
}

#[tokio::test]
async fn matched_element_with_empty_text_remains_successful() {
    let (client, server) = mock_client(json!({"result":{
        "type":"object","subtype":"node","className":"HTMLDivElement","objectId":"empty-node"
    }})).await;
    let text = super::super::get_element_text(&client, "page", &RefMap::new(), "#empty", &HashMap::new()).await.unwrap();
    assert_eq!(text, "");
    assert_eq!(server.await.unwrap(), ["Runtime.evaluate", "Runtime.callFunctionOn"]);
}

#[tokio::test]
async fn unmatched_selector_remains_an_element_not_found_error() {
    let (client, server) = mock_client(json!({"result":{"type":"object","subtype":"null","value":null}})).await;
    let error = super::super::get_element_text(&client, "page", &RefMap::new(), "#missing", &HashMap::new()).await.unwrap_err();
    assert!(error.contains("Element not found: #missing"), "{error}");
    assert_eq!(server.await.unwrap(), ["Runtime.evaluate"]);
}

#[tokio::test]
async fn cross_origin_frame_selector_exception_is_not_an_element_handle() {
    set_active_frame(Some("frame"));
    let (client, server) = mock_client(selector_exception()).await;
    let frames = HashMap::from([("frame".to_string(), "frame-session".to_string())]);
    let result = super::super::get_element_text(&client, "page", &RefMap::new(), "[", &frames).await;
    set_active_frame(None);
    assert!(result.unwrap_err().contains("SyntaxError: invalid selector"));
    assert_eq!(server.await.unwrap(), ["Runtime.evaluate"]);
}

#[tokio::test]
async fn same_process_frame_selector_exception_is_not_an_element_handle() {
    set_active_frame(Some("frame"));
    let (client, server) = mock_client(selector_exception()).await;
    let result = super::super::get_element_text(&client, "page", &RefMap::new(), "[", &HashMap::new()).await;
    set_active_frame(None);
    assert!(result.unwrap_err().contains("SyntaxError: invalid selector"));
    assert_eq!(server.await.unwrap(), ["DOM.getFrameOwner", "DOM.resolveNode", "Runtime.callFunctionOn"]);
}

#[tokio::test]
async fn exception_object_without_exception_details_is_not_an_element_handle() {
    let mut response = selector_exception();
    response.as_object_mut().unwrap().remove("exceptionDetails");
    let (client, server) = mock_client(response).await;
    let result = super::super::get_element_text(&client, "page", &RefMap::new(), "[", &HashMap::new()).await;
    assert!(result.unwrap_err().contains("SyntaxError: invalid selector"));
    assert_eq!(server.await.unwrap(), ["Runtime.evaluate"]);
}
