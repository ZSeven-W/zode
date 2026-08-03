//! In-conversation find bar (Codex `threadFindBar` parity): a floating strip
//! docked to the top of the active session's transcript column with a query
//! field, an `N/M` counter, previous/next steppers and a close button.
//!
//! Geometry is derived from the transcript content rect the shell already
//! computes, the same way `AnchorRail` derives the gutter rail - no
//! `WorkspaceLayout` field is added for it. The bar floats *over* the
//! transcript rather than shrinking it so paint, hit testing and the
//! accessibility tree keep agreeing on one `layout.transcript`; the
//! transcript's own jump-to-match scroll leaves room above the target item.

use jian_core::text_input::TextInputState;
use jian_widgets::{components::text_input::TextInputView, HorizontalAlign, Painter, Rect};
use zode_app_model::{AppCommand, TranscriptFindState, TranscriptState, ZodeAppState};
use zode_node_protocol::SessionLocator;

use crate::{
    paint_elevated_surface, paint_single_line, GlobalSearchController, GlobalSearchOutcome,
    IconButton, RectExt, SemanticIcon, WidgetId, ZodeTheme,
};

pub const TRANSCRIPT_FIND_SURFACE_ID: WidgetId = WidgetId(270);
pub const TRANSCRIPT_FIND_INPUT_ID: WidgetId = WidgetId(271);
pub const TRANSCRIPT_FIND_PREVIOUS_ID: WidgetId = WidgetId(272);
pub const TRANSCRIPT_FIND_NEXT_ID: WidgetId = WidgetId(273);
pub const TRANSCRIPT_FIND_CLOSE_ID: WidgetId = WidgetId(274);

/// The find field is a plain single-line text input with exactly the caret,
/// selection and IME behavior the global-search field already implements, so
/// it reuses that controller verbatim rather than growing a second copy that
/// would drift.
pub type TranscriptFindController = GlobalSearchController;
pub type TranscriptFindOutcome = GlobalSearchOutcome;

pub const FIND_BAR_HEIGHT: f32 = 36.0;
const SURFACE_MAX_W: f32 = 420.0;
const SURFACE_MIN_W: f32 = 240.0;
const SURFACE_TOP_GAP: f32 = 4.0;
const SURFACE_PAD_X: f32 = 6.0;
const BUTTON_SIZE: f32 = 24.0;
const BUTTON_GAP: f32 = 2.0;
const COUNTER_W: f32 = 46.0;
const COUNTER_GAP: f32 = 6.0;
const ICON_SIZE: f32 = 14.0;
const INPUT_MIN_W: f32 = 80.0;
const RADIUS: f32 = 8.0;

/// Placement for every part of the bar. Shared by paint, hit testing and the
/// accessibility tree so all three agree on exactly one geometry.
#[derive(Debug, Clone, PartialEq)]
pub struct TranscriptFindBarLayout {
    pub surface: Rect,
    pub input: Rect,
    pub counter: Rect,
    /// Rendered `N/M` text, already resolved against the current matches.
    pub counter_label: String,
    pub previous: Rect,
    pub next: Rect,
    pub close: Rect,
    /// `false` while nothing matches - the steppers still paint (so the bar
    /// does not reflow as the user types) but read as disabled and refuse to
    /// produce a command.
    pub navigable: bool,
}

pub struct TranscriptFindBar;

impl TranscriptFindBar {
    /// The active session plus its transcript and find state, or `None` when
    /// no session is selected, its transcript is missing, or the bar is
    /// closed. Every entry point below funnels through this so the widget
    /// can never paint against a session the shell is not showing.
    pub fn active(
        state: &ZodeAppState,
    ) -> Option<(&SessionLocator, &TranscriptState, &TranscriptFindState)> {
        let session = state.current_session.as_ref()?;
        let transcript = state.transcripts.get(session)?;
        let find = &state.presentation.sessions.get(session)?.find;
        find.open.then_some((session, transcript, find))
    }

    pub fn layout(transcript_rect: Rect, state: &ZodeAppState) -> Option<TranscriptFindBarLayout> {
        let (_, transcript, find) = Self::active(state)?;
        if transcript_rect.size.x <= 0.0 || transcript_rect.size.y < FIND_BAR_HEIGHT {
            return None;
        }
        let width = SURFACE_MAX_W.min(transcript_rect.size.x);
        if width < SURFACE_MIN_W {
            return None;
        }
        // Right-aligned against the transcript column, matching where the
        // reference docks it and keeping it clear of the anchor rail gutter.
        let surface = Rect::xywh(
            transcript_rect.max_x() - width,
            transcript_rect.origin.y + SURFACE_TOP_GAP,
            width,
            FIND_BAR_HEIGHT,
        );
        let mut trailing = surface.max_x() - SURFACE_PAD_X;
        let button_y = surface.origin.y + (surface.size.y - BUTTON_SIZE) / 2.0;
        let place_button = |trailing: &mut f32| {
            *trailing -= BUTTON_SIZE;
            let rect = Rect::xywh(*trailing, button_y, BUTTON_SIZE, BUTTON_SIZE);
            *trailing -= BUTTON_GAP;
            rect
        };
        let close = place_button(&mut trailing);
        let next = place_button(&mut trailing);
        let previous = place_button(&mut trailing);
        trailing -= COUNTER_GAP;
        let counter = Rect::xywh(
            trailing - COUNTER_W,
            surface.origin.y,
            COUNTER_W,
            surface.size.y,
        );
        let input_x = surface.origin.x + SURFACE_PAD_X;
        let input_w = (counter.origin.x - COUNTER_GAP - input_x).max(INPUT_MIN_W);
        let input = Rect::xywh(input_x, surface.origin.y, input_w, surface.size.y);
        Some(TranscriptFindBarLayout {
            surface,
            input,
            counter,
            counter_label: find.counter_label(transcript),
            previous,
            next,
            close,
            navigable: find.match_count(transcript) > 0,
        })
    }

    /// Resolves a click on one of the bar's controls. The input field itself
    /// carries no command - focusing it is the host's job, exactly like
    /// `GLOBAL_SEARCH_INPUT_ID`.
    pub fn command_for_widget(state: &ZodeAppState, id: WidgetId) -> Option<AppCommand> {
        let (session, transcript, find) = Self::active(state)?;
        if id == TRANSCRIPT_FIND_CLOSE_ID {
            return Some(AppCommand::CloseTranscriptFind {
                session: session.clone(),
            });
        }
        let forward = match id {
            TRANSCRIPT_FIND_NEXT_ID => true,
            TRANSCRIPT_FIND_PREVIOUS_ID => false,
            _ => return None,
        };
        (find.match_count(transcript) > 0).then(|| AppCommand::StepTranscriptFindMatch {
            session: session.clone(),
            forward,
        })
    }

    pub fn paint(
        painter: &mut dyn Painter,
        layout: &TranscriptFindBarLayout,
        input: &TextInputState,
        focused: Option<WidgetId>,
        hovered: Option<WidgetId>,
        theme: &ZodeTheme,
    ) {
        paint_elevated_surface(painter, layout.surface, RADIUS, theme);
        painter.fill_round_rect(layout.surface, RADIUS, theme.tokens.popover);
        painter.stroke_round_rect(layout.surface, RADIUS, theme.tokens.border, 1.0);
        painter.save();
        painter.clip_rect(layout.surface);
        painter.stroke_svg_path(
            SemanticIcon::Search.path(),
            jian_widgets::Point2D::new(
                layout.input.origin.x + 4.0,
                layout.input.origin.y + (layout.input.size.y - ICON_SIZE) / 2.0,
            ),
            ICON_SIZE,
            theme.tokens.muted_foreground,
            SemanticIcon::Search.stroke_width(),
        );
        TextInputView {
            state: input,
            placeholder: "在对话中查找",
            focused: focused == Some(TRANSCRIPT_FIND_INPUT_ID),
            font_size: 13.0,
            now_ms: 0,
            pad_x: 4.0 + ICON_SIZE + 6.0,
            baseline_delta_y: 0.0,
            mask: None,
        }
        .paint(painter, layout.input, &theme.tokens);
        paint_single_line(
            painter,
            &layout.counter_label,
            layout.counter,
            12.0,
            400,
            theme.tokens.muted_foreground,
            HorizontalAlign::End,
        );
        for (rect, icon, id) in [
            (
                layout.previous,
                SemanticIcon::MoveUp,
                TRANSCRIPT_FIND_PREVIOUS_ID,
            ),
            (layout.next, SemanticIcon::MoveDown, TRANSCRIPT_FIND_NEXT_ID),
            (layout.close, SemanticIcon::Close, TRANSCRIPT_FIND_CLOSE_ID),
        ] {
            let enabled = layout.navigable || id == TRANSCRIPT_FIND_CLOSE_ID;
            IconButton::paint(
                painter,
                rect,
                icon,
                ICON_SIZE,
                enabled && hovered == Some(id),
                &theme.tokens,
            );
            if !enabled {
                // `IconButton` always strokes with `tokens.foreground`; repaint
                // the glyph muted so a stepper that cannot act reads disabled
                // rather than merely un-hovered.
                painter.stroke_svg_path(
                    icon.path(),
                    jian_widgets::Point2D::new(
                        rect.origin.x + (rect.size.x - ICON_SIZE) / 2.0,
                        rect.origin.y + (rect.size.y - ICON_SIZE) / 2.0,
                    ),
                    ICON_SIZE,
                    theme.tokens.popover,
                    icon.stroke_width() + 1.0,
                );
                painter.stroke_svg_path(
                    icon.path(),
                    jian_widgets::Point2D::new(
                        rect.origin.x + (rect.size.x - ICON_SIZE) / 2.0,
                        rect.origin.y + (rect.size.y - ICON_SIZE) / 2.0,
                    ),
                    ICON_SIZE,
                    theme.tokens.muted_foreground.with_alpha(0.4),
                    icon.stroke_width(),
                );
            }
        }
        painter.restore();
    }
}

#[cfg(test)]
mod tests {
    use zode_app_model::{demo_state, TranscriptItem, TranscriptState};
    use zode_node_protocol::{SessionLocator, ThreadStatus, ThreadSummary, WorkspaceUri};

    use super::*;

    fn state_with_open_find(query: &str) -> (ZodeAppState, SessionLocator) {
        let mut state = demo_state();
        let session = SessionLocator::new(state.host.node_id, "task-1");
        state.threads.push(ThreadSummary {
            session: session.clone(),
            workspace_uri: WorkspaceUri::new("file:///repo/zode").unwrap(),
            title: "Task".into(),
            updated_at_ms: 0,
            status: ThreadStatus::Idle,
        });
        state.transcripts.insert(
            session.clone(),
            TranscriptState {
                items: vec![
                    TranscriptItem::user_text("hit"),
                    TranscriptItem::assistant_text("hit hit"),
                ],
                ..TranscriptState::default()
            },
        );
        state.current_session = Some(session.clone());
        let find = &mut state
            .presentation
            .sessions
            .entry(session.clone())
            .or_default()
            .find;
        find.open = true;
        find.query = query.to_owned();
        (state, session)
    }

    fn transcript_rect() -> Rect {
        Rect::xywh(100.0, 60.0, 600.0, 400.0)
    }

    #[test]
    fn a_closed_bar_has_no_layout() {
        let (mut state, session) = state_with_open_find("hit");
        state
            .presentation
            .sessions
            .get_mut(&session)
            .unwrap()
            .find
            .open = false;
        assert!(TranscriptFindBar::layout(transcript_rect(), &state).is_none());
    }

    #[test]
    fn controls_are_laid_out_in_order_inside_the_surface() {
        let (state, _) = state_with_open_find("hit");
        let layout = TranscriptFindBar::layout(transcript_rect(), &state).unwrap();

        assert_eq!(layout.surface.max_x(), transcript_rect().max_x());
        assert!(layout.input.max_x() <= layout.counter.origin.x);
        assert!(layout.counter.max_x() <= layout.previous.origin.x);
        assert!(layout.previous.max_x() <= layout.next.origin.x);
        assert!(layout.next.max_x() <= layout.close.origin.x);
        assert!(layout.close.max_x() <= layout.surface.max_x());
        assert!(layout.surface.size.y == FIND_BAR_HEIGHT);
    }

    #[test]
    fn the_counter_reports_the_live_match_position() {
        let (state, _) = state_with_open_find("hit");
        let layout = TranscriptFindBar::layout(transcript_rect(), &state).unwrap();
        assert_eq!(layout.counter_label, "1/3");
        assert!(layout.navigable);
    }

    #[test]
    fn a_query_with_no_matches_disables_navigation() {
        let (state, _) = state_with_open_find("missing");
        let layout = TranscriptFindBar::layout(transcript_rect(), &state).unwrap();
        assert_eq!(layout.counter_label, "0/0");
        assert!(!layout.navigable);
        assert!(
            TranscriptFindBar::command_for_widget(&state, TRANSCRIPT_FIND_NEXT_ID).is_none(),
            "a stepper with nothing to step through must not emit a command"
        );
        assert!(matches!(
            TranscriptFindBar::command_for_widget(&state, TRANSCRIPT_FIND_CLOSE_ID),
            Some(AppCommand::CloseTranscriptFind { .. })
        ));
    }

    #[test]
    fn steppers_map_to_forward_and_backward_commands() {
        let (state, session) = state_with_open_find("hit");
        assert_eq!(
            TranscriptFindBar::command_for_widget(&state, TRANSCRIPT_FIND_NEXT_ID),
            Some(AppCommand::StepTranscriptFindMatch {
                session: session.clone(),
                forward: true,
            })
        );
        assert_eq!(
            TranscriptFindBar::command_for_widget(&state, TRANSCRIPT_FIND_PREVIOUS_ID),
            Some(AppCommand::StepTranscriptFindMatch {
                session,
                forward: false,
            })
        );
    }

    #[test]
    fn a_transcript_column_too_narrow_for_the_bar_hides_it() {
        let (state, _) = state_with_open_find("hit");
        assert!(TranscriptFindBar::layout(Rect::xywh(0.0, 0.0, 180.0, 400.0), &state).is_none());
    }
}
