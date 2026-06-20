//! Design-pipeline orchestrator: schedules op's design_skeleton/content/refine.

use serde_json::{json, Value};

use super::OpError;

/// A section: its skeleton spec (sent to design_skeleton) + the content intent
/// (consumed by the content step, NOT sent to skeleton).
#[derive(Debug, Clone)]
pub struct SectionPlan {
    pub skeleton: Value,
    pub content_intent: String,
}

/// The plan the content generator produces; maps to design_skeleton args.
#[derive(Debug, Clone)]
pub struct DesignPlan {
    pub root_frame: Value,
    pub sections: Vec<SectionPlan>,
    pub style_guide: Option<Value>,
    /// op's Rust parser reads canvasWidth as a positive integer.
    pub canvas_width: Option<u32>,
    pub page_id: Option<String>,
}

impl DesignPlan {
    /// Build `design_skeleton` args (`{rootFrame, sections, styleGuide?, canvasWidth?, pageId?}`).
    /// Per-section `content_intent` is NOT forwarded — it is consumed locally.
    pub fn to_skeleton_args(&self) -> Value {
        let mut args = json!({
            "rootFrame": self.root_frame,
            "sections": self.sections.iter().map(|s| s.skeleton.clone()).collect::<Vec<_>>(),
        });
        if let Some(sg) = &self.style_guide {
            args["styleGuide"] = sg.clone();
        }
        if let Some(w) = self.canvas_width {
            // Serialize as an integer, not a float — op's parser is strict.
            args["canvasWidth"] = json!(w);
        }
        if let Some(p) = &self.page_id {
            args["pageId"] = json!(p);
        }
        args
    }
}

/// Normalized design_skeleton result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skeleton {
    pub root_id: String,
    pub section_ids: Vec<String>,
}

/// Parse op's Rust design_skeleton result: `{rootId, sectionIds:"a,b,c", sections:"<json>"}`.
/// Returns `OpError::Parse` if `rootId` is absent.
pub fn normalize_skeleton(result: &Value) -> Result<Skeleton, OpError> {
    let root_id = result
        .get("rootId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| OpError::Parse("design_skeleton: no rootId".into()))?
        .to_string();

    let section_ids = result
        .get("sectionIds")
        .and_then(|v| v.as_str())
        .map(|s| {
            s.split(',')
                .filter(|x| !x.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();

    Ok(Skeleton {
        root_id,
        section_ids,
    })
}

/// Outcome summary after the full design pipeline completes.
#[derive(Debug, Clone)]
pub struct DesignResult {
    pub section_ids: Vec<String>,
    pub refine: Value,
    pub failures: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn to_skeleton_args_matches_op_schema() {
        let plan = DesignPlan {
            root_frame: json!({"name":"root","width":1200,"height":800}),
            sections: vec![
                SectionPlan {
                    skeleton: json!({"name":"Header","height":80}),
                    content_intent: "logo + nav".into(),
                },
                SectionPlan {
                    skeleton: json!({"name":"Hero"}),
                    content_intent: "headline".into(),
                },
            ],
            style_guide: None,
            canvas_width: Some(1200),
            page_id: None,
        };
        let args = plan.to_skeleton_args();
        assert_eq!(args["rootFrame"]["width"], 1200);
        assert_eq!(args["sections"].as_array().unwrap().len(), 2);
        assert_eq!(args["sections"][0]["name"], "Header");
        assert_eq!(args["canvasWidth"], 1200);
        assert!(args.get("styleGuide").is_none()); // omitted when None
        assert!(args["sections"][0].get("content_intent").is_none()); // intent NOT sent to skeleton
    }

    #[test]
    fn normalize_skeleton_parses_rust_result() {
        let raw = json!({"rootId":"10","sectionIds":"11,12,13","sections":"[{\"id\":\"11\"}]"});
        let s = normalize_skeleton(&raw).unwrap();
        assert_eq!(s.root_id, "10");
        assert_eq!(s.section_ids, vec!["11", "12", "13"]);
    }

    #[test]
    fn normalize_skeleton_errors_without_root() {
        assert!(normalize_skeleton(&json!({"sectionIds":"1"})).is_err());
    }
}
