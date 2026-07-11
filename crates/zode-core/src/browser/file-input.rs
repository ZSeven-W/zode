use super::backend::{BrowserError, ClickTarget};

pub(crate) fn expression(target: &ClickTarget, multiple: bool) -> Result<String, BrowserError> {
    let (lookup, missing) = match target {
        ClickTarget::Selector(selector) => (
            format!(
                "document.querySelector({})",
                serde_json::to_string(selector)
                    .map_err(|error| BrowserError::Protocol(error.to_string()))?
            ),
            "no match",
        ),
        ClickTarget::Ref(reference) => (
            format!("document.querySelector('[data-zode-ref=\"{reference}\"]')"),
            "stale ref",
        ),
        ClickTarget::Coords { .. } => {
            return Err(BrowserError::Protocol(
                "upload target requires selector or ref".into(),
            ));
        }
    };
    Ok(format!(
        "(()=>{{const e={lookup};if(!e)throw new Error('{missing}');if(!(e instanceof HTMLInputElement)||e.type!=='file')throw new Error('wrong element type');if({multiple}&&!e.multiple)throw new Error('file input does not allow multiple files');return e}})()"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ref_expression_reports_stale_and_checks_type_and_multiple() {
        let expression = expression(&ClickTarget::Ref(7), true).unwrap();
        assert!(expression.contains("stale ref"));
        assert!(expression.contains("HTMLInputElement"));
        assert!(expression.contains("!e.multiple"));
    }
}
