use jian_widgets::{Color, Painter, Point2D, Rect, TextLayout};
use zode_app_model::{
    demo_state, integration_catalog, AppCommand, IntegrationScope, IntegrationsTab, LoadState,
    ProjectState, ShellRoute,
};
use zode_app_ui::{Insets, IntegrationsPage, RectExt, WorkspaceSnapshot, ZodeTheme};
use zode_node_protocol::{
    IntegrationRegistryEntry, IntegrationRegistryKind, IntegrationRegistrySnapshot,
    IntegrationRegistryState, WorkspaceUri,
};

#[derive(Default)]
struct PaintCapture {
    texts: Vec<String>,
    rounded_fills: Vec<Rect>,
    clips: Vec<Rect>,
}

impl Painter for PaintCapture {
    fn begin_frame(&mut self) {}
    fn end_frame(&mut self) {}
    fn fill_rect(&mut self, _rect: Rect, _color: Color) {}
    fn stroke_rect(&mut self, _rect: Rect, _color: Color, _width: f32) {}
    fn draw_text(&mut self, layout: &TextLayout, _origin: Point2D) {
        self.texts.push(
            layout
                .runs()
                .iter()
                .map(|run| run.content.as_str())
                .collect(),
        );
    }
    fn clip_rect(&mut self, rect: Rect) {
        self.clips.push(rect);
    }
    fn stroke_line(&mut self, _from: Point2D, _to: Point2D, _color: Color, _width: f32) {}
    fn fill_round_rect(&mut self, rect: Rect, _radius: f32, _color: Color) {
        self.rounded_fills.push(rect);
    }
    fn stroke_round_rect(&mut self, _rect: Rect, _radius: f32, _color: Color, _width: f32) {}
    fn stroke_svg_path(
        &mut self,
        _d: &str,
        _top_left: Point2D,
        _size: f32,
        _color: Color,
        _width: f32,
    ) {
    }
    fn save(&mut self) {}
    fn restore(&mut self) {}
    fn translate(&mut self, _offset: Point2D) {}
    fn resize(&mut self, _width: u32, _height: u32) {}
    fn dpi_scale(&self) -> f32 {
        1.0
    }
    fn measure_text_weighted(&mut self, text: &str, font_size: f32, _weight: u16) -> f32 {
        text.chars().count() as f32 * font_size * 0.55
    }
}

fn raw_entry(
    source_id: &str,
    name: &str,
    kind: IntegrationRegistryKind,
    state: IntegrationRegistryState,
    installed: bool,
) -> IntegrationRegistryEntry {
    IntegrationRegistryEntry {
        source_id: source_id.into(),
        name: name.into(),
        description: format!("{name} description"),
        kind,
        state,
        installed,
    }
}

fn catalog_state(tab: IntegrationsTab) -> zode_app_model::ZodeAppState {
    let mut state = demo_state();
    state.presentation.route = ShellRoute::Integrations(tab);
    let workspace_uri = WorkspaceUri::new("file:///repo/zode").unwrap();
    state.projects.push(ProjectState {
        workspace_uri: workspace_uri.clone(),
        expanded: true,
        available: true,
        last_opened_ms: 1,
    });
    state.active_workspace = Some(workspace_uri.clone());
    let mut entries = [
        "filesystem",
        "search",
        "shell",
        "git",
        "web",
        "notebook",
        "todo",
        "subagent",
        "op",
        "browser",
    ]
    .into_iter()
    .map(|name| {
        raw_entry(
            &format!("tools:{name}"),
            name,
            IntegrationRegistryKind::ToolGroup,
            IntegrationRegistryState::Ready,
            true,
        )
    })
    .collect::<Vec<_>>();
    entries.extend([
        raw_entry(
            "capability:agent",
            "智能体",
            IntegrationRegistryKind::NodeCapability,
            IntegrationRegistryState::Ready,
            true,
        ),
        raw_entry(
            "capability:workspace",
            "工作区",
            IntegrationRegistryKind::NodeCapability,
            IntegrationRegistryState::Ready,
            true,
        ),
        raw_entry(
            "skill:review",
            "review",
            IntegrationRegistryKind::Skill,
            IntegrationRegistryState::Disabled,
            true,
        ),
        raw_entry(
            "mcp:github",
            "github",
            IntegrationRegistryKind::Mcp,
            IntegrationRegistryState::Configured,
            false,
        ),
        raw_entry(
            "lsp:rust",
            "rust",
            IntegrationRegistryKind::Lsp,
            IntegrationRegistryState::Configured,
            false,
        ),
    ]);
    state.presentation.integrations =
        LoadState::Ready(integration_catalog(IntegrationRegistrySnapshot {
            workspace_uri,
            entries,
            directory_error: Some("在线目录不可用；当前仅显示本机已发现的集成。".into()),
        }));
    state
}

#[test]
fn wide_page_freezes_reference_column_and_exposes_real_tab_commands() {
    let state = catalog_state(IntegrationsTab::Plugins);
    let surface = Rect::xywh(240.0, 0.0, 1_560.0, 1_080.0);

    let layout = IntegrationsPage::layout(surface, &state);

    assert_eq!(layout.content, Rect::xywh(652.0, 46.0, 736.0, 1_034.0));
    assert_eq!(layout.search, Rect::xywh(652.0, 146.0, 736.0, 34.0));
    assert_eq!(layout.tabs[0].label, "插件");
    assert!(layout.tabs[0].selected);
    assert_eq!(layout.tabs[1].label, "技能");
    assert_eq!(
        IntegrationsPage::command_for_widget(&state, layout.tabs[1].id),
        Some(AppCommand::SelectIntegrationsTab(IntegrationsTab::Skills)),
    );
}

#[test]
fn production_catalog_meets_truthful_landmarks_and_uses_non_empty_monograms() {
    let state = catalog_state(IntegrationsTab::Plugins);
    let catalog = state.presentation.integrations.ready().unwrap();

    assert!(catalog.installed.len() >= 8);
    assert!(catalog.sections.len() >= 2);
    assert!(catalog.all_entries().count() >= 10);
    assert!(catalog
        .all_entries()
        .all(|entry| entry.source_id.is_some() && !entry.fixture_only));
    assert!(catalog
        .all_entries()
        .all(|entry| !entry.icon.label().is_empty()));
    assert!(catalog.all_entries().all(|entry| {
        entry.category != zode_app_model::IntegrationCategory::Mcp
            || matches!(
                entry.availability,
                zode_app_model::Availability::Configured | zode_app_model::Availability::Disabled
            )
    }));
}

#[test]
fn installed_strip_and_catalog_rows_share_stable_source_backed_layouts() {
    let state = catalog_state(IntegrationsTab::Plugins);
    let surface = Rect::xywh(240.0, 0.0, 1_560.0, 1_080.0);
    let installed = IntegrationsPage::installed_icon_layout(surface, &state);
    let sections = IntegrationsPage::catalog_section_layout(surface, &state);

    assert!(installed.len() >= 8);
    assert!(sections.len() >= 2);
    assert!(
        sections
            .iter()
            .map(|section| section.rows.len())
            .sum::<usize>()
            >= 10
    );
    for icon in &installed {
        assert!(!icon.source_id.is_empty());
        assert!(!icon.monogram.is_empty());
        assert!(icon.rect.size.x > 0.0 && icon.rect.size.y > 0.0);
    }
    for row in sections.iter().flat_map(|section| &section.rows) {
        assert!(!row.source_id.is_empty());
        assert_eq!(
            IntegrationsPage::row_widget_id(&row.source_id),
            row.id,
            "paint/a11y identity must derive from the registry source id",
        );
    }
}

#[test]
fn paint_renders_installed_categories_and_honest_states_without_marketplace_claims() {
    let state = catalog_state(IntegrationsTab::Plugins);
    let surface = Rect::xywh(240.0, 0.0, 1_560.0, 1_080.0);
    let mut painter = PaintCapture::default();

    IntegrationsPage::paint(&mut painter, surface, &state, &ZodeTheme::light());

    let text = painter.texts.join("\n");
    for expected in [
        "插件",
        "在常用工具与 Zode 协作",
        "搜索插件或技能",
        "个人",
        "已安装",
        "内置工具",
        "节点能力",
        "github",
        "已配置",
        "在线目录不可用；当前仅显示本机已发现的集成。",
    ] {
        assert!(text.contains(expected), "missing paint text: {expected}");
    }
    for forbidden in ["connected", "已连接", "Featured", "安装 51"] {
        assert!(!text.contains(forbidden), "fabricated state: {forbidden}");
    }
    assert!(painter.clips.contains(&surface));
}

#[test]
fn skills_tab_filters_rows_but_keeps_the_real_installed_strip() {
    let state = catalog_state(IntegrationsTab::Skills);
    let surface = Rect::xywh(240.0, 0.0, 1_560.0, 1_080.0);

    let sections = IntegrationsPage::catalog_section_layout(surface, &state);
    let installed = IntegrationsPage::installed_icon_layout(surface, &state);

    assert_eq!(sections.len(), 1);
    assert_eq!(sections[0].title, "技能");
    assert_eq!(sections[0].rows.len(), 1);
    assert_eq!(sections[0].rows[0].source_id, "skill:review");
    assert!(installed.len() >= 8);
}

#[test]
fn accessibility_uses_row_rects_and_exposes_only_real_actions() {
    let state = catalog_state(IntegrationsTab::Plugins);
    let snapshot = WorkspaceSnapshot::build(&state, 1_800.0, 1_080.0, Insets::ZERO);
    let sections =
        IntegrationsPage::catalog_section_layout(snapshot.layout.primary_surface, &state);
    let first = sections
        .iter()
        .flat_map(|section| &section.rows)
        .next()
        .unwrap();
    let node = snapshot.node(first.id).expect("catalog row is accessible");

    assert_eq!(node.rect, first.rect);
    assert!(node.name.contains(&first.name));
    assert!(node.actions.is_empty());
    assert!(node.focus_order.is_none());
    assert_eq!(
        snapshot.hit_test(Point2D::new(
            first.rect.origin.x + first.rect.size.x / 2.0,
            first.rect.origin.y + first.rect.size.y / 2.0,
        )),
        None
    );

    let action = snapshot
        .node(first.action_id)
        .expect("toggle action is accessible");
    assert!(action.actions.contains(&accesskit::Action::Click));
    assert!(!action.disabled);
    assert_eq!(
        IntegrationsPage::command_for_widget(&state, first.action_id),
        Some(AppCommand::SetIntegrationEnabled {
            workspace_uri: WorkspaceUri::new("file:///repo/zode").unwrap(),
            source_id: first.source_id.clone(),
            enabled: false,
        })
    );
}

#[test]
fn search_and_public_personal_filters_never_invent_directory_entries() {
    let mut state = catalog_state(IntegrationsTab::Plugins);
    let surface = Rect::xywh(240.0, 0.0, 1_560.0, 1_080.0);

    state.presentation.integration_search = "github".into();
    let sections = IntegrationsPage::catalog_section_layout(surface, &state);
    assert_eq!(
        sections
            .iter()
            .flat_map(|section| &section.rows)
            .map(|row| row.source_id.as_str())
            .collect::<Vec<_>>(),
        vec!["mcp:github"]
    );

    state.presentation.integration_search.clear();
    state.presentation.integration_scope = IntegrationScope::Public;
    assert!(IntegrationsPage::catalog_section_layout(surface, &state).is_empty());
    let mut painter = PaintCapture::default();
    IntegrationsPage::paint(&mut painter, surface, &state, &ZodeTheme::light());
    assert!(painter
        .texts
        .join("\n")
        .contains("当前节点没有可验证的可安装项目"));

    assert_eq!(
        IntegrationsPage::command_for_widget(&state, zode_app_ui::INTEGRATIONS_PERSONAL_SCOPE_ID,),
        Some(AppCommand::SetIntegrationScope(IntegrationScope::Personal))
    );
}

#[test]
fn loading_failed_and_idle_states_are_explicit_and_never_fabricate_rows() {
    for (load, expected) in [
        (LoadState::Idle, "尚未加载本机集成"),
        (LoadState::Loading, "正在读取本机集成…"),
        (
            LoadState::Failed("permission denied".into()),
            "permission denied",
        ),
    ] {
        let mut state = demo_state();
        state.presentation.route = ShellRoute::Integrations(IntegrationsTab::Plugins);
        state.presentation.integrations = load;
        let surface = Rect::xywh(0.0, 0.0, 736.0, 720.0);
        let mut painter = PaintCapture::default();

        IntegrationsPage::paint(&mut painter, surface, &state, &ZodeTheme::light());

        assert!(painter.texts.join("\n").contains(expected));
        assert!(IntegrationsPage::installed_icon_layout(surface, &state).is_empty());
        assert!(IntegrationsPage::catalog_section_layout(surface, &state).is_empty());
    }
}

#[test]
fn narrow_layout_never_produces_negative_or_overlapping_row_columns() {
    let state = catalog_state(IntegrationsTab::Plugins);
    let surface = Rect::xywh(0.0, 0.0, 420.0, 720.0);
    let sections = IntegrationsPage::catalog_section_layout(surface, &state);

    for section in sections {
        for row in &section.rows {
            assert!(row.rect.size.x >= 0.0 && row.rect.size.y > 0.0);
            assert!(row.rect.min_x() >= surface.min_x());
            assert!(row.rect.max_x() <= surface.max_x());
        }
        for pair in section.rows.windows(2) {
            assert!(pair[0].rect.max_y() <= pair[1].rect.min_y());
        }
    }
}

#[test]
fn integration_static_ids_do_not_overlap_adjacent_shell_components() {
    let integration_ids = [
        zode_app_ui::INTEGRATIONS_SEARCH_ID,
        zode_app_ui::INTEGRATIONS_PUBLIC_SCOPE_ID,
        zode_app_ui::INTEGRATIONS_PERSONAL_SCOPE_ID,
    ];
    let reserved = [
        zode_app_ui::PANEL_PICKER_ID,
        zode_app_ui::SECONDARY_HOME_REVIEW_ID,
        zode_app_ui::SECONDARY_HOME_TERMINAL_ID,
        zode_app_ui::SECONDARY_HOME_BROWSER_ID,
        zode_app_ui::SECONDARY_HOME_FILES_ID,
        zode_app_ui::SECONDARY_HOME_SIDE_TASK_ID,
        zode_app_ui::EMPTY_SUGGESTION_IDS[0],
        zode_app_ui::EMPTY_SUGGESTION_IDS[1],
        zode_app_ui::EMPTY_SUGGESTION_IDS[2],
        zode_app_ui::EMPTY_SUGGESTION_IDS[3],
    ];
    assert!(integration_ids
        .iter()
        .all(|id| !reserved.iter().any(|reserved| reserved == id)));
}
