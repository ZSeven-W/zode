//! Page-load outcome taxonomy.
//!
//! CDP reports navigation failures as a single `errorText` string
//! (`net::ERR_NAME_NOT_RESOLVED`, `net::ERR_CONNECTION_REFUSED`, …) and
//! reports HTTP error statuses not at all — a 404 is a perfectly
//! successful navigation. Both backends funnel their navigation result
//! through [`NavigationOutcome`] so `browser_act navigate` hands the model
//! a stable, documented class string it can branch on instead of a bare
//! "here is the current URL".

use serde::{Serialize, Serializer};

/// How a page load ended. The string form (see [`LoadClass::as_str`]) is
/// the stable contract surfaced to the model as the `class` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadClass {
    /// Navigation committed and the browser reported no error.
    Ok,
    /// Host name could not be resolved.
    DnsFailure,
    /// The host actively refused the connection.
    ConnectionRefused,
    /// Connection failed for another transport reason (reset, closed,
    /// unreachable address, empty response).
    ConnectionFailed,
    /// No network at all — the machine is offline or the network changed
    /// mid-flight.
    Offline,
    /// Certificate / TLS handshake problem.
    TlsError,
    /// The configured proxy could not be used.
    ProxyError,
    /// The page loaded, but the main document's HTTP status was >= 400.
    /// The code is carried in [`NavigationOutcome::http_status`].
    HttpError,
    /// The load did not finish inside the navigation timeout. This is not
    /// necessarily fatal — check [`NavigationOutcome::loaded`], which is
    /// true when a document is nevertheless present (slow-but-loading).
    Timeout,
    /// Navigation was cancelled (superseded by another navigation, or the
    /// response turned out to be a download).
    Aborted,
    /// Blocked by policy: an extension, CSP, admin policy, or an unsafe
    /// port.
    Blocked,
    /// The URL itself was rejected (malformed, unknown scheme).
    InvalidUrl,
    /// Navigation failed for a reason we do not classify; `detail` carries
    /// the raw text.
    Unknown,
}

impl LoadClass {
    /// Stable identifier surfaced to the model.
    pub fn as_str(self) -> &'static str {
        match self {
            LoadClass::Ok => "ok",
            LoadClass::DnsFailure => "dns_failure",
            LoadClass::ConnectionRefused => "connection_refused",
            LoadClass::ConnectionFailed => "connection_failed",
            LoadClass::Offline => "offline",
            LoadClass::TlsError => "tls_error",
            LoadClass::ProxyError => "proxy_error",
            LoadClass::HttpError => "http_error",
            LoadClass::Timeout => "timeout",
            LoadClass::Aborted => "aborted",
            LoadClass::Blocked => "blocked",
            LoadClass::InvalidUrl => "invalid_url",
            LoadClass::Unknown => "unknown",
        }
    }

    /// Human-readable one-liner for approval prompts and error messages.
    pub fn describe(self) -> &'static str {
        match self {
            LoadClass::Ok => "loaded",
            LoadClass::DnsFailure => "DNS resolution failed",
            LoadClass::ConnectionRefused => "connection refused",
            LoadClass::ConnectionFailed => "connection failed",
            LoadClass::Offline => "no network connection",
            LoadClass::TlsError => "TLS/certificate error",
            LoadClass::ProxyError => "proxy error",
            LoadClass::HttpError => "HTTP error status",
            LoadClass::Timeout => "load timed out",
            LoadClass::Aborted => "navigation aborted",
            LoadClass::Blocked => "navigation blocked",
            LoadClass::InvalidUrl => "invalid URL",
            LoadClass::Unknown => "navigation failed",
        }
    }

    pub fn is_ok(self) -> bool {
        matches!(self, LoadClass::Ok)
    }
}

impl Serialize for LoadClass {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

/// Maps a CDP/Chromium navigation `errorText` to a [`LoadClass`].
///
/// Chromium's net error names are matched as substrings so the same table
/// works for a bare `net::ERR_…`, a chromiumoxide `ChromeMessage(…)`
/// wrapper, or the extension bridge's raw `errorText`. Order matters:
/// the more specific families (proxy, timeout) are tested before the
/// broader `ERR_CONNECTION_*` family they would otherwise fall into.
pub fn classify_net_error(text: &str) -> LoadClass {
    let t = text.to_ascii_uppercase();
    let has = |needle: &str| t.contains(needle);

    if has("ERR_NAME_NOT_RESOLVED")
        || has("ERR_NAME_RESOLUTION_FAILED")
        || has("ERR_DNS_")
        || has("ERR_ICANN_NAME_COLLISION")
    {
        return LoadClass::DnsFailure;
    }
    if has("ERR_INTERNET_DISCONNECTED")
        || has("ERR_NETWORK_CHANGED")
        || has("ERR_NETWORK_IO_SUSPENDED")
    {
        return LoadClass::Offline;
    }
    if has("ERR_PROXY")
        || has("ERR_TUNNEL_CONNECTION_FAILED")
        || has("ERR_UNEXPECTED_PROXY_AUTH")
        || has("ERR_MANDATORY_PROXY_CONFIGURATION_FAILED")
    {
        return LoadClass::ProxyError;
    }
    if has("ERR_CERT") || has("ERR_SSL") || has("ERR_TLS") || has("ERR_QUIC_HANDSHAKE_FAILED") {
        return LoadClass::TlsError;
    }
    if has("_TIMED_OUT") {
        // ERR_TIMED_OUT and ERR_CONNECTION_TIMED_OUT — the latter would
        // otherwise be swallowed by the ERR_CONNECTION_* family below.
        // (ERR_DNS_TIMED_OUT already matched as a DNS failure.)
        return LoadClass::Timeout;
    }
    if has("ERR_CONNECTION_REFUSED") {
        return LoadClass::ConnectionRefused;
    }
    if has("ERR_CONNECTION_")
        || has("ERR_ADDRESS_UNREACHABLE")
        || has("ERR_ADDRESS_INVALID")
        || has("ERR_SOCKET_NOT_CONNECTED")
        || has("ERR_EMPTY_RESPONSE")
        || has("ERR_RESPONSE_HEADERS_TRUNCATED")
    {
        return LoadClass::ConnectionFailed;
    }
    if has("ERR_BLOCKED_BY")
        || has("ERR_UNSAFE_PORT")
        || has("ERR_UNSAFE_REDIRECT")
        || has("ERR_ACCESS_DENIED")
    {
        return LoadClass::Blocked;
    }
    if has("ERR_ABORTED") {
        return LoadClass::Aborted;
    }
    if has("ERR_INVALID_URL") || has("ERR_UNKNOWN_URL_SCHEME") || has("ERR_DISALLOWED_URL_SCHEME") {
        return LoadClass::InvalidUrl;
    }
    LoadClass::Unknown
}

/// Pulls the bare `net::ERR_…` token out of a longer error string, so
/// the detail we report is the browser's own error name rather than
/// whatever wrapper text the CDP client added around it.
pub fn net_error_token(text: &str) -> Option<&str> {
    let start = text.find("net::ERR_")?;
    let rest = &text[start..];
    let end = rest
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == ':'))
        .unwrap_or(rest.len());
    Some(&rest[..end])
}

/// Result of a navigation attempt: where the browser ended up, plus how
/// the load ended.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NavigationOutcome {
    /// URL after the attempt. On a hard failure this is typically the
    /// page the browser was already on, not the requested URL.
    pub url: String,
    pub class: LoadClass,
    /// Main-document HTTP status when it could be correlated from the
    /// network log (best effort — absent for `data:`/`about:` URLs and
    /// when the response event has not been observed).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_status: Option<u16>,
    /// Raw browser-supplied detail, e.g. `net::ERR_NAME_NOT_RESOLVED`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// Whether a document is present after the attempt. True for every
    /// successful load, and for a timeout on a page that did commit — the
    /// "slow but loading" case callers must not treat as fatal.
    pub loaded: bool,
}

impl NavigationOutcome {
    /// A clean load of `url`.
    pub fn ok(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            class: LoadClass::Ok,
            http_status: None,
            detail: None,
            loaded: true,
        }
    }

    /// A classified failure that left the browser at `url`.
    pub fn failed(url: impl Into<String>, class: LoadClass, detail: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            class,
            http_status: None,
            detail: Some(detail.into()),
            loaded: false,
        }
    }

    pub fn is_ok(&self) -> bool {
        self.class.is_ok()
    }

    /// One-line human summary, used as the tool result's `message`.
    pub fn message(&self) -> String {
        if self.is_ok() {
            return match self.http_status {
                Some(code) => format!("loaded {} ({code})", self.url),
                None => format!("loaded {}", self.url),
            };
        }
        let mut msg = format!("{} ({})", self.class.describe(), self.class.as_str());
        if let Some(code) = self.http_status {
            msg.push_str(&format!(" {code}"));
        }
        if let Some(detail) = &self.detail {
            msg.push_str(&format!(": {detail}"));
        }
        if self.loaded {
            msg.push_str("; a document is present — the page may still be loading");
        }
        msg
    }

    /// Tool-result shape for `browser_act navigate`. `url` is kept as the
    /// first-class field it has always been; `ok`/`class` are what the
    /// model branches on.
    pub fn to_json(&self) -> serde_json::Value {
        let mut v = serde_json::json!({
            "url": self.url,
            "ok": self.is_ok(),
            "class": self.class,
            "loaded": self.loaded,
            "message": self.message(),
        });
        if let Some(code) = self.http_status {
            v["http_status"] = serde_json::json!(code);
        }
        if let Some(detail) = &self.detail {
            v["detail"] = serde_json::json!(detail);
        }
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_text_maps_to_classes() {
        // (errorText, expected class) — the documented mapping table.
        let table = [
            ("net::ERR_NAME_NOT_RESOLVED", LoadClass::DnsFailure),
            ("net::ERR_NAME_RESOLUTION_FAILED", LoadClass::DnsFailure),
            ("net::ERR_DNS_TIMED_OUT", LoadClass::DnsFailure),
            ("net::ERR_CONNECTION_REFUSED", LoadClass::ConnectionRefused),
            ("net::ERR_CONNECTION_RESET", LoadClass::ConnectionFailed),
            ("net::ERR_CONNECTION_CLOSED", LoadClass::ConnectionFailed),
            ("net::ERR_ADDRESS_UNREACHABLE", LoadClass::ConnectionFailed),
            ("net::ERR_EMPTY_RESPONSE", LoadClass::ConnectionFailed),
            ("net::ERR_INTERNET_DISCONNECTED", LoadClass::Offline),
            ("net::ERR_NETWORK_CHANGED", LoadClass::Offline),
            ("net::ERR_CERT_AUTHORITY_INVALID", LoadClass::TlsError),
            ("net::ERR_CERT_DATE_INVALID", LoadClass::TlsError),
            ("net::ERR_SSL_PROTOCOL_ERROR", LoadClass::TlsError),
            ("net::ERR_PROXY_CONNECTION_FAILED", LoadClass::ProxyError),
            ("net::ERR_TUNNEL_CONNECTION_FAILED", LoadClass::ProxyError),
            ("net::ERR_TIMED_OUT", LoadClass::Timeout),
            ("net::ERR_CONNECTION_TIMED_OUT", LoadClass::Timeout),
            ("net::ERR_ABORTED", LoadClass::Aborted),
            ("net::ERR_BLOCKED_BY_CLIENT", LoadClass::Blocked),
            ("net::ERR_BLOCKED_BY_RESPONSE", LoadClass::Blocked),
            ("net::ERR_UNSAFE_PORT", LoadClass::Blocked),
            ("net::ERR_INVALID_URL", LoadClass::InvalidUrl),
            ("net::ERR_UNKNOWN_URL_SCHEME", LoadClass::InvalidUrl),
            ("something else entirely", LoadClass::Unknown),
        ];
        for (text, want) in table {
            assert_eq!(classify_net_error(text), want, "for {text}");
        }
    }

    #[test]
    fn classification_survives_wrapper_text_and_case() {
        // chromiumoxide surfaces errorText inside its own Display output.
        assert_eq!(
            classify_net_error("Chrome message: net::ERR_CONNECTION_REFUSED"),
            LoadClass::ConnectionRefused
        );
        assert_eq!(
            classify_net_error("net::err_name_not_resolved"),
            LoadClass::DnsFailure
        );
    }

    #[test]
    fn connection_aborted_is_not_the_aborted_class() {
        // ERR_CONNECTION_ABORTED is a transport failure; only the bare
        // ERR_ABORTED means "navigation cancelled".
        assert_eq!(
            classify_net_error("net::ERR_CONNECTION_ABORTED"),
            LoadClass::ConnectionFailed
        );
    }

    #[test]
    fn net_error_token_is_extracted_from_wrapper_text() {
        assert_eq!(
            net_error_token("Chrome message: net::ERR_CONNECTION_REFUSED"),
            Some("net::ERR_CONNECTION_REFUSED")
        );
        assert_eq!(
            net_error_token("net::ERR_ABORTED at frame 3"),
            Some("net::ERR_ABORTED")
        );
        assert_eq!(net_error_token("websocket closed"), None);
    }

    #[test]
    fn ok_outcome_serializes_with_url_first_class() {
        let v = NavigationOutcome::ok("https://x.test/").to_json();
        assert_eq!(v["url"], "https://x.test/");
        assert_eq!(v["ok"], true);
        assert_eq!(v["class"], "ok");
        assert_eq!(v["loaded"], true);
        assert!(v.get("detail").is_none());
    }

    #[test]
    fn failed_outcome_carries_class_and_detail() {
        let v = NavigationOutcome::failed(
            "about:blank",
            LoadClass::DnsFailure,
            "net::ERR_NAME_NOT_RESOLVED",
        )
        .to_json();
        assert_eq!(v["ok"], false);
        assert_eq!(v["class"], "dns_failure");
        assert_eq!(v["loaded"], false);
        assert_eq!(v["detail"], "net::ERR_NAME_NOT_RESOLVED");
        assert!(v["message"].as_str().unwrap().contains("DNS resolution"));
    }

    #[test]
    fn http_error_reports_the_code() {
        let mut o = NavigationOutcome::ok("https://x.test/missing");
        o.class = LoadClass::HttpError;
        o.http_status = Some(404);
        let v = o.to_json();
        assert_eq!(v["class"], "http_error");
        assert_eq!(v["http_status"], 404);
        // The document exists even though the status is an error.
        assert_eq!(v["loaded"], true);
    }

    #[test]
    fn timeout_with_document_says_still_loading() {
        let mut o = NavigationOutcome::failed("https://slow.test/", LoadClass::Timeout, "10s");
        o.loaded = true;
        let msg = o.message();
        assert!(msg.contains("timed out"), "{msg}");
        assert!(msg.contains("still be loading"), "{msg}");
    }
}
