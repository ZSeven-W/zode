use super::*;

use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicBool, Ordering};

use agent::file_cache::FileStateCache;
use agent::hook::HookRunner;
use agent::permission::PermissionManager;
use agent::testing::MockProvider;
use agent::tool::{SafetyClass, Tool, ToolUseContext};
use agent_tools_code::TaskAgentFactory;
use async_trait::async_trait;
use serde_json::{json, Value};

#[derive(Debug)]
struct ClassifiedStubTool(&'static str, SafetyClass);

#[async_trait]
impl Tool for ClassifiedStubTool {
    fn name(&self) -> &str {
        self.0
    }

    fn description(&self) -> &str {
        "classified stub"
    }

    fn input_schema(&self) -> Value {
        json!({"type": "object"})
    }

    fn safety_class(&self) -> SafetyClass {
        self.1
    }

    async fn call(&self, _ctx: &ToolUseContext, _input: Value) -> Result<Value, AgentError> {
        Ok(json!({}))
    }
}

fn factory_with_prompt(
    cell: ParentToolsCell,
    defs: Vec<crate::agents::AgentDef>,
    skills_prompt: String,
) -> Arc<ZodeTaskFactory> {
    Arc::new(ZodeTaskFactory::new(
        ModelRuntimeState::new(
            Arc::new(MockProvider::new(Vec::new())),
            "parent-model".into(),
        ),
        Arc::new(PermissionManager::new()),
        std::env::temp_dir(),
        Arc::new(FileStateCache::new(
            NonZeroUsize::new(8).unwrap(),
            1024 * 1024,
        )),
        Arc::new(HookRunner::new()),
        cell,
        defs,
        skills_prompt,
        crate::subagents::SubAgentRegistry::new(),
        None,
    ))
}

fn factory_with(cell: ParentToolsCell, defs: Vec<crate::agents::AgentDef>) -> Arc<ZodeTaskFactory> {
    factory_with_prompt(cell, defs, String::new())
}

#[tokio::test]
async fn inherited_children_keep_task_but_exclude_team_and_host_control_tools() {
    let mut parent = ToolRegistry::new();
    let parent_task: Arc<dyn Tool> = Arc::new(ClassifiedStubTool("Task", SafetyClass::Mutating));
    parent.register(parent_task.clone());
    parent.register(Arc::new(ClassifiedStubTool(
        "ToolSearch",
        SafetyClass::ReadOnly,
    )));
    parent.register(Arc::new(ClassifiedStubTool(
        "FileRead",
        SafetyClass::ReadOnly,
    )));
    for name in TEAM_TOOL_NAMES {
        parent.register(Arc::new(ClassifiedStubTool(name, SafetyClass::ReadOnly)));
    }
    for name in CHILD_HOST_CONTROL_TOOLS {
        parent.register(Arc::new(ClassifiedStubTool(name, SafetyClass::ReadOnly)));
    }
    let cell: ParentToolsCell = Arc::new(OnceLock::new());
    cell.set(Arc::new(parent)).unwrap();

    let cfg = factory_with(cell, Vec::new())
        .build("researcher")
        .await
        .unwrap();
    let child_task = cfg.tools.get("Task").expect("child keeps Task");
    assert!(Arc::ptr_eq(&parent_task, &child_task));
    assert!(cfg.tools.get("FileRead").is_some());
    for name in TEAM_TOOL_NAMES
        .iter()
        .chain(CHILD_HOST_CONTROL_TOOLS.iter())
    {
        assert!(cfg.tools.get(name).is_none(), "child leaked {name}");
    }

    let search = cfg.tools.get("ToolSearch").unwrap();
    let search_result = search
        .call(
            &ToolUseContext::new(std::env::temp_dir()),
            json!({"query": "select:Task,TeamHire,GoalComplete,AskUserQuestion,FileRead"}),
        )
        .await
        .unwrap();
    assert_eq!(search_result["matches"], json!(["Task", "FileRead"]));
    assert_eq!(
        search_result["missing"],
        json!(["TeamHire", "GoalComplete", "AskUserQuestion"])
    );
}

#[test]
fn teammate_tools_block_direct_and_workflow_mediated_task_dispatch() {
    let mut parent = ToolRegistry::new();
    parent.register(Arc::new(ClassifiedStubTool("Task", SafetyClass::Mutating)));
    parent.register(Arc::new(ClassifiedStubTool(
        "RunWorkflow",
        SafetyClass::Mutating,
    )));
    parent.register(Arc::new(ClassifiedStubTool(
        "FileRead",
        SafetyClass::ReadOnly,
    )));
    let cell: ParentToolsCell = Arc::new(OnceLock::new());
    cell.set(Arc::new(parent)).unwrap();

    let teammate_tools = shared_child_tools(&cell, &TEAM_TOOL_NAMES_WITH_TASK);

    assert!(teammate_tools.get("Task").is_none());
    assert!(teammate_tools.get("RunWorkflow").is_none());
    assert!(teammate_tools.get("FileRead").is_some());
}

#[tokio::test]
async fn child_does_not_restore_parent_filtered_discovery_surfaces() {
    let mut parent = ToolRegistry::new();
    parent.register(Arc::new(ClassifiedStubTool(
        "FileRead",
        SafetyClass::ReadOnly,
    )));
    let cell: ParentToolsCell = Arc::new(OnceLock::new());
    cell.set(Arc::new(parent)).unwrap();

    let child = shared_child_tools(&cell, &[]);

    assert!(child.get("FileRead").is_some());
    assert!(child.get("ToolSearch").is_none());
    let cfg = factory_with_prompt(
        cell,
        Vec::new(),
        "\n## Available Skills\n- filtered-out: must not leak\n".into(),
    )
    .build("general")
    .await
    .unwrap();
    let system = cfg.system.unwrap();
    assert!(!system.contains("filtered-out"));
    assert!(!system.contains("Registered MCP tools inherited"));
}

#[tokio::test]
async fn additional_host_agent_types_are_advertised_to_nested_children() {
    let factory = factory_with(Arc::new(OnceLock::new()), Vec::new());
    factory.set_additional_agent_types(vec![(
        "external-reviewer".into(),
        "External CLI review profile".into(),
    )]);

    let cfg = factory.build("general").await.unwrap();
    let system = cfg.system.unwrap();

    assert!(system.contains("- external-reviewer: External CLI review profile"));
}

#[tokio::test]
async fn child_inherits_registered_skills_and_mcp_and_can_discover_them() {
    let skills = agent::skills::SkillRegistry::new();
    skills.insert(agent::skills::Skill {
        name: "security-audit".into(),
        description: "Audit a change for security issues".into(),
        prompt: "Inspect {target} carefully.".into(),
        model: None,
        allow_tools: Default::default(),
        input_schema: json!({
            "type": "object",
            "properties": {"target": {"type": "string"}},
            "required": ["target"]
        }),
    });
    let skill_tool: Arc<dyn Tool> =
        Arc::new(crate::skills::SkillTool::new(Arc::new(skills.clone())));
    let raw_mcp = Arc::new(agent::testing::FakeTool::new(
        "mcp__registered__inspect",
        json!({"inspected": true}),
    ));
    let mcp_tool: Arc<dyn Tool> = Arc::new(crate::gated_tool::PermissionGatedTool::new(
        raw_mcp.clone(),
        Arc::new(crate::approval::BypassGate),
    ));
    let mut parent = ToolRegistry::new();
    parent.register(skill_tool.clone());
    parent.register(mcp_tool.clone());
    parent.register(Arc::new(ClassifiedStubTool(
        "ToolSearch",
        SafetyClass::ReadOnly,
    )));
    let cell: ParentToolsCell = Arc::new(OnceLock::new());
    cell.set(Arc::new(parent)).unwrap();
    let factory = factory_with_prompt(
        cell,
        Vec::new(),
        crate::skills::skills_prompt(&skills, true),
    );

    let cfg = factory.build("general").await.unwrap();

    let child_skill = cfg.tools.get("Skill").expect("Skill inherited");
    assert!(Arc::ptr_eq(&skill_tool, &child_skill));
    let child_mcp = cfg
        .tools
        .get("mcp__registered__inspect")
        .expect("MCP inherited");
    assert!(Arc::ptr_eq(&mcp_tool, &child_mcp));
    let system = cfg.system.unwrap();
    assert!(system.contains("- security-audit: Audit a change for security issues"));
    assert!(system.contains("invoke it with the Skill tool FIRST"));
    assert!(system.contains("Registered MCP tools inherited from the caller"));

    let skill_output = child_skill
        .call(
            &ToolUseContext::new(std::env::temp_dir()),
            json!({"name": "security-audit", "params": {"target": "Task"}}),
        )
        .await
        .unwrap();
    assert_eq!(skill_output["instructions"], "Inspect Task carefully.");
    let mcp_output = child_mcp
        .call(
            &ToolUseContext::new(std::env::temp_dir()),
            json!({"resource": "Task"}),
        )
        .await
        .unwrap();
    assert_eq!(mcp_output["inspected"], true);
    assert_eq!(raw_mcp.call_count(), 1);

    let search = cfg.tools.get("ToolSearch").unwrap();
    let search_output = search
        .call(
            &ToolUseContext::new(std::env::temp_dir()),
            json!({"query": "select:Skill,mcp__registered__inspect"}),
        )
        .await
        .unwrap();
    assert_eq!(
        search_output["matches"],
        json!(["Skill", "mcp__registered__inspect"])
    );
}

#[tokio::test]
async fn read_only_child_modes_keep_skills_but_drop_unknown_mcp_tools() {
    let mut parent = ToolRegistry::new();
    parent.register(Arc::new(ClassifiedStubTool("Skill", SafetyClass::ReadOnly)));
    parent.register(Arc::new(ClassifiedStubTool(
        "mcp__registered__mutability_unknown",
        SafetyClass::Unknown,
    )));
    parent.register(Arc::new(ClassifiedStubTool(
        "ToolSearch",
        SafetyClass::ReadOnly,
    )));
    let cell: ParentToolsCell = Arc::new(OnceLock::new());
    cell.set(Arc::new(parent)).unwrap();
    let factory = factory_with_prompt(
        cell,
        Vec::new(),
        "\n## Available Skills\n- safe-skill: read-only guidance\n".into(),
    );

    for mode in [SubagentMode::Plan, SubagentMode::ReadOnly] {
        let cfg = ZodeTaskModeFactory::new(factory.clone(), mode)
            .build("general")
            .await
            .unwrap();

        assert!(cfg.tools.get("Skill").is_some(), "mode={}", mode.as_str());
        assert!(
            cfg.tools
                .get("mcp__registered__mutability_unknown")
                .is_none(),
            "mode={}",
            mode.as_str()
        );
        let system = cfg.system.unwrap();
        assert!(system.contains("- safe-skill:"), "mode={}", mode.as_str());
        assert!(
            !system.contains("Registered MCP tools inherited"),
            "mode={}",
            mode.as_str()
        );
    }
}

#[tokio::test]
async fn plan_mode_narrows_parent_ceiling_and_rebuilds_tool_search() {
    let mut parent = ToolRegistry::new();
    let lsp_manager = Arc::new(crate::lsp::LspManager::new(
        crate::config::LspConfig::default(),
        std::env::temp_dir(),
    ));
    for tool in crate::lsp::lsp_tools(&lsp_manager) {
        parent.register(tool);
    }
    let goal_completed = Arc::new(AtomicBool::new(false));
    parent.register(Arc::new(crate::goal::GoalCompleteTool::new(
        goal_completed.clone(),
        crate::verification::VerificationState::default(),
        false,
    )));
    let (question_queue, _question_receiver) = crate::question::question_queue();
    parent.register(Arc::new(crate::question::AskUserQuestionTool::new(
        question_queue,
        None,
    )));
    parent.register(Arc::new(agent_tools_code::BashOutputTool::new(
        agent_tools_code::BashSessionRegistry::new(),
    )));
    parent.register(Arc::new(ClassifiedStubTool(
        "ToolSearch",
        SafetyClass::ReadOnly,
    )));
    parent.register(Arc::new(ClassifiedStubTool("Task", SafetyClass::Mutating)));
    parent.register(Arc::new(ClassifiedStubTool(
        "FileRead",
        SafetyClass::ReadOnly,
    )));
    parent.register(Arc::new(ClassifiedStubTool(
        "FileWrite",
        SafetyClass::Mutating,
    )));
    parent.register(Arc::new(ClassifiedStubTool(
        "Unclassified",
        SafetyClass::Unknown,
    )));
    let cell: ParentToolsCell = Arc::new(OnceLock::new());
    cell.set(Arc::new(parent)).unwrap();

    let mode_factory = ZodeTaskModeFactory::new(factory_with(cell, Vec::new()), SubagentMode::Plan);
    let cfg = mode_factory.build("general").await.unwrap();

    assert!(cfg
        .tools
        .list()
        .iter()
        .all(|tool| matches!(tool.safety_class(), SafetyClass::ReadOnly)));
    assert!(cfg.tools.get("FileRead").is_some());
    assert!(cfg.tools.get("Task").is_none());
    assert!(cfg.tools.get("FileWrite").is_none());
    assert!(cfg.tools.get("Unclassified").is_none());
    assert!(cfg.tools.get("GoalComplete").is_none());
    assert!(cfg.tools.get("AskUserQuestion").is_none());
    assert!(cfg.tools.get("BashOutput").is_none());
    assert!(cfg.tools.get("LspDiagnostics").is_none());
    assert!(!goal_completed.load(Ordering::SeqCst));

    let search = cfg.tools.get("ToolSearch").unwrap();
    let search_result = search
        .call(
            &ToolUseContext::new(std::env::temp_dir()),
            json!({
                "query": "select:Task,FileRead,FileWrite,Unclassified,GoalComplete,AskUserQuestion,BashOutput,LspDiagnostics"
            }),
        )
        .await
        .unwrap();
    assert_eq!(search_result["matches"], json!(["FileRead"]));
    assert_eq!(
        search_result["missing"],
        json!([
            "Task",
            "FileWrite",
            "Unclassified",
            "GoalComplete",
            "AskUserQuestion",
            "BashOutput",
            "LspDiagnostics"
        ])
    );

    let system = cfg.system.unwrap();
    assert!(system.contains("PLAN MODE for this delegated task"));
    assert!(system.contains("return a concise"));
    assert!(system.contains("calling agent"));
    assert!(!system.contains("/plan"));
    assert!(!system.contains("Available sub-agent types"));
    assert!(!system.contains("You may use the Task tool"));
}

#[tokio::test]
async fn read_only_mode_narrows_tools_without_forcing_plan_behavior() {
    let mut parent = ToolRegistry::new();
    parent.register(Arc::new(ClassifiedStubTool(
        "FileRead",
        SafetyClass::ReadOnly,
    )));
    parent.register(Arc::new(ClassifiedStubTool(
        "FileWrite",
        SafetyClass::Mutating,
    )));
    parent.register(Arc::new(ClassifiedStubTool("Task", SafetyClass::Mutating)));
    let cell: ParentToolsCell = Arc::new(OnceLock::new());
    cell.set(Arc::new(parent)).unwrap();
    let mode_factory =
        ZodeTaskModeFactory::new(factory_with(cell, Vec::new()), SubagentMode::ReadOnly);

    let cfg = mode_factory.build("general").await.unwrap();

    assert!(cfg.tools.get("FileRead").is_some());
    assert!(cfg.tools.get("FileWrite").is_none());
    assert!(cfg.tools.get("Task").is_none());
    let system = cfg.system.unwrap();
    assert!(!system.contains("PLAN MODE"));
    assert!(!system.contains("return a concise, concrete implementation plan"));
}

#[tokio::test]
async fn custom_agent_named_plan_cannot_override_plan_mode_policy() {
    let mut parent = ToolRegistry::new();
    parent.register(Arc::new(ClassifiedStubTool(
        "FileRead",
        SafetyClass::ReadOnly,
    )));
    parent.register(Arc::new(ClassifiedStubTool(
        "FileWrite",
        SafetyClass::Mutating,
    )));
    let cell: ParentToolsCell = Arc::new(OnceLock::new());
    cell.set(Arc::new(parent)).unwrap();
    let factory = factory_with(
        cell,
        vec![crate::agents::AgentDef {
            name: "plan".into(),
            description: "custom planner with a tempting mode name".into(),
            system: "CUSTOM PLAN PERSONA".into(),
            model: Some("custom-model".into()),
        }],
    );
    let mode_factory = ZodeTaskModeFactory::new(factory, SubagentMode::Plan);

    let cfg = mode_factory.build("plan").await.unwrap();

    assert_eq!(cfg.model, "custom-model");
    assert!(cfg.system.unwrap().contains("CUSTOM PLAN PERSONA"));
    assert!(cfg.tools.get("FileRead").is_some());
    assert!(cfg.tools.get("FileWrite").is_none());
}
