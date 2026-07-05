use super::{BridgeServer, RpcKind};
use crate::browser::backend::{
    BrowserBackend, BrowserError, ClickTarget, ConsoleEntry, NetworkEntry, Screenshot, TabInfo,
};
use crate::browser::snapshot_js::SNAPSHOT_JS;
use async_trait::async_trait;
use base64::Engine as _;
use std::sync::Arc;

#[derive(Debug)]
pub struct BridgeBackend {
    server: Arc<BridgeServer>,
}

impl BridgeBackend {
    pub fn new(server: Arc<BridgeServer>) -> Arc<Self> {
        Arc::new(Self { server })
    }

    async fn cdp(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, BrowserError> {
        self.server.call(RpcKind::Cdp, method, params).await
    }

    async fn ext(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, BrowserError> {
        self.server.call(RpcKind::Ext, method, params).await
    }
}

#[async_trait]
impl BrowserBackend for BridgeBackend {
    async fn navigate(&self, url: &str) -> Result<String, BrowserError> {
        self.cdp("Page.navigate", serde_json::json!({ "url": url }))
            .await?;
        self.current_url().await
    }

    async fn screenshot(&self) -> Result<Screenshot, BrowserError> {
        let payload = self
            .cdp(
                "Page.captureScreenshot",
                serde_json::json!({ "format": "jpeg", "quality": 70 }),
            )
            .await?;
        screenshot_from_payload(&payload)
    }

    async fn snapshot(&self) -> Result<String, BrowserError> {
        let payload = self
            .cdp(
                "Runtime.evaluate",
                serde_json::json!({ "expression": SNAPSHOT_JS, "returnByValue": true }),
            )
            .await?;
        let value = runtime_value(&payload);
        Ok(value
            .get("outline")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string())
    }

    async fn click(&self, target: &ClickTarget) -> Result<(), BrowserError> {
        match target {
            ClickTarget::Selector(sel) => {
                self.cdp(
                    "Runtime.evaluate",
                    serde_json::json!({ "expression": js_click_selector(sel), "returnByValue": true }),
                )
                .await?;
            }
            ClickTarget::Ref(n) => {
                self.cdp(
                    "Runtime.evaluate",
                    serde_json::json!({ "expression": js_click_ref(*n), "returnByValue": true }),
                )
                .await?;
            }
            ClickTarget::Coords { x, y } => {
                self.cdp(
                    "Input.dispatchMouseEvent",
                    serde_json::json!({
                        "type": "mousePressed",
                        "x": x,
                        "y": y,
                        "button": "left",
                        "clickCount": 1
                    }),
                )
                .await?;
                self.cdp(
                    "Input.dispatchMouseEvent",
                    serde_json::json!({
                        "type": "mouseReleased",
                        "x": x,
                        "y": y,
                        "button": "left",
                        "clickCount": 1
                    }),
                )
                .await?;
            }
        }
        Ok(())
    }

    async fn type_text(&self, target: &ClickTarget, text: &str) -> Result<(), BrowserError> {
        match target {
            ClickTarget::Selector(sel) => {
                self.cdp(
                    "Runtime.evaluate",
                    serde_json::json!({ "expression": js_focus_selector(sel), "returnByValue": true }),
                )
                .await?;
            }
            ClickTarget::Ref(n) => {
                self.cdp(
                    "Runtime.evaluate",
                    serde_json::json!({ "expression": js_focus_ref(*n), "returnByValue": true }),
                )
                .await?;
            }
            ClickTarget::Coords { .. } => {
                self.click(target).await?;
            }
        }
        self.cdp("Input.insertText", serde_json::json!({ "text": text }))
            .await?;
        Ok(())
    }

    async fn press_key(&self, key: &str) -> Result<(), BrowserError> {
        self.cdp(
            "Input.dispatchKeyEvent",
            serde_json::json!({ "type": "keyDown", "key": key }),
        )
        .await?;
        self.cdp(
            "Input.dispatchKeyEvent",
            serde_json::json!({ "type": "keyUp", "key": key }),
        )
        .await?;
        Ok(())
    }

    async fn scroll(&self, dx: f64, dy: f64) -> Result<(), BrowserError> {
        self.cdp(
            "Runtime.evaluate",
            serde_json::json!({
                "expression": format!("window.scrollBy({dx},{dy})"),
                "returnByValue": true
            }),
        )
        .await?;
        Ok(())
    }

    async fn evaluate(&self, expression: &str) -> Result<serde_json::Value, BrowserError> {
        let payload = self
            .cdp(
                "Runtime.evaluate",
                serde_json::json!({ "expression": expression, "returnByValue": true }),
            )
            .await?;
        Ok(runtime_value(&payload).clone())
    }

    async fn console_logs(&self, limit: usize) -> Result<Vec<ConsoleEntry>, BrowserError> {
        let payload = self
            .ext("logs.console", serde_json::json!({ "limit": limit }))
            .await?;
        Ok(parse_console(&payload))
    }

    async fn network_log(&self, limit: usize) -> Result<Vec<NetworkEntry>, BrowserError> {
        let payload = self
            .ext("logs.network", serde_json::json!({ "limit": limit }))
            .await?;
        Ok(parse_network(&payload))
    }

    async fn tabs(&self) -> Result<Vec<TabInfo>, BrowserError> {
        let payload = self.ext("tabs.list", serde_json::json!({})).await?;
        parse_tabs(&payload)
    }

    async fn tab_new(&self, url: Option<&str>) -> Result<TabInfo, BrowserError> {
        let payload = self
            .ext("tabs.new", serde_json::json!({ "url": url }))
            .await?;
        parse_tab(&payload)
    }

    async fn tab_close(&self, id: &str) -> Result<(), BrowserError> {
        self.ext("tabs.close", serde_json::json!({ "id": id }))
            .await?;
        Ok(())
    }

    async fn tab_select(&self, id: &str) -> Result<(), BrowserError> {
        self.ext("tabs.select", serde_json::json!({ "id": id }))
            .await?;
        Ok(())
    }

    async fn current_url(&self) -> Result<String, BrowserError> {
        let payload = self.ext("tabs.current", serde_json::json!({})).await?;
        payload
            .as_str()
            .map(ToString::to_string)
            .ok_or_else(|| BrowserError::Protocol("bridge tabs.current returned non-string".into()))
    }

    async fn is_alive(&self) -> bool {
        self.server.is_connected()
    }

    async fn close(&self) -> Result<(), BrowserError> {
        self.ext("debugger.detach", serde_json::json!({})).await?;
        Ok(())
    }
}

fn js_click_selector(sel: &str) -> String {
    format!(
        "(()=>{{const e=document.querySelector({});if(!e)throw new Error('no match');e.click();return true}})()",
        js_string_expr(sel)
    )
}

fn js_click_ref(n: u32) -> String {
    format!(
        "(()=>{{const e=document.querySelector('[data-zode-ref=\"{}\"]');if(!e)throw new Error('no ref');e.click();return true}})()",
        n
    )
}

fn js_focus_selector(sel: &str) -> String {
    format!(
        "(()=>{{const e=document.querySelector({});if(!e)throw new Error('no match');e.focus();return true}})()",
        js_string_expr(sel)
    )
}

fn js_string_expr(s: &str) -> String {
    if s.is_empty() {
        return "\"\"".into();
    }
    let codes = s
        .chars()
        .map(|c| u32::from(c).to_string())
        .collect::<Vec<_>>()
        .join(",");
    format!("String.fromCodePoint({codes})")
}

fn js_focus_ref(n: u32) -> String {
    format!(
        "(()=>{{const e=document.querySelector('[data-zode-ref=\"{}\"]');if(!e)throw new Error('no ref');e.focus();return true}})()",
        n
    )
}

fn runtime_value(payload: &serde_json::Value) -> &serde_json::Value {
    payload
        .get("result")
        .and_then(|r| r.get("value"))
        .unwrap_or(payload)
}

fn parse_console(payload: &serde_json::Value) -> Vec<ConsoleEntry> {
    serde_json::from_value(payload.clone()).unwrap_or_default()
}

fn parse_network(payload: &serde_json::Value) -> Vec<NetworkEntry> {
    serde_json::from_value(payload.clone()).unwrap_or_default()
}

fn parse_tabs(payload: &serde_json::Value) -> Result<Vec<TabInfo>, BrowserError> {
    serde_json::from_value(payload.clone())
        .map_err(|e| BrowserError::Protocol(format!("bridge tabs payload: {e}")))
}

fn parse_tab(payload: &serde_json::Value) -> Result<TabInfo, BrowserError> {
    serde_json::from_value(payload.clone())
        .map_err(|e| BrowserError::Protocol(format!("bridge tab payload: {e}")))
}

fn screenshot_from_payload(payload: &serde_json::Value) -> Result<Screenshot, BrowserError> {
    let data = payload
        .get("data")
        .and_then(|v| v.as_str())
        .ok_or_else(|| BrowserError::Protocol("bridge screenshot missing data".into()))?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .map_err(|e| BrowserError::Protocol(format!("bridge screenshot base64: {e}")))?;
    Ok(Screenshot {
        bytes,
        media_type: "image/jpeg",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn click_selector_js_escapes_quotes() {
        let js = js_click_selector("a[href=\"/x\"]");
        assert!(js.contains("querySelector"));
        assert!(!js.contains("\"a[href="));
    }

    #[test]
    fn parse_console_entries_from_ext_payload() {
        let v = serde_json::json!([{"level":"error","text":"boom"}]);
        let entries = parse_console(&v);
        assert_eq!(
            entries,
            vec![ConsoleEntry {
                level: "error".into(),
                text: "boom".into()
            }]
        );
    }

    #[test]
    fn screenshot_decodes_base64() {
        let v = serde_json::json!({ "data": "AQ==" });
        let shot = screenshot_from_payload(&v).unwrap();
        assert_eq!(shot.bytes, vec![1u8]);
        assert_eq!(shot.media_type, "image/jpeg");
    }
}
