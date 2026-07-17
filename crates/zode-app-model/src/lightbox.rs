use zode_node_protocol::SessionLocator;

/// Discrete zoom levels the lightbox stepper cycles through, expressed as
/// whole percent of the image's natural size. `100` is exact native size.
/// Kept as a fixed table (rather than free-form scroll-to-zoom) so the
/// stepper UI and its accessibility label always agree on a small, stable
/// set of values - matching the reference design's "-"/"+"/percent-readout
/// control.
pub const LIGHTBOX_ZOOM_STEPS: [u32; 8] = [25, 50, 65, 75, 100, 125, 150, 200];

/// Zoom step index a lightbox opens at - the reference design's initial
/// "65%" fit-to-view zoom.
pub const LIGHTBOX_DEFAULT_ZOOM_INDEX: usize = 2;

/// One open full-size image overlay. `item_id` addresses the `ImageItem`
/// inside `session`'s transcript being previewed; `reduce_task_navigation`
/// refuses to open a lightbox for a missing item and the reducer never lets
/// `item_id` point past a transcript that no longer contains it (see
/// `session_has_image_item`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LightboxState {
    pub session: SessionLocator,
    pub item_id: String,
    pub zoom_index: usize,
}

impl LightboxState {
    pub fn new(session: SessionLocator, item_id: String) -> Self {
        Self {
            session,
            item_id,
            zoom_index: LIGHTBOX_DEFAULT_ZOOM_INDEX,
        }
    }

    pub fn zoom_percent(&self) -> u32 {
        LIGHTBOX_ZOOM_STEPS[self.zoom_index.min(LIGHTBOX_ZOOM_STEPS.len() - 1)]
    }

    /// Steps to the next/previous zoom level, saturating at the ends of
    /// `LIGHTBOX_ZOOM_STEPS` rather than wrapping.
    pub fn step_zoom(&mut self, increase: bool) {
        let last = LIGHTBOX_ZOOM_STEPS.len() - 1;
        self.zoom_index = if increase {
            (self.zoom_index + 1).min(last)
        } else {
            self.zoom_index.saturating_sub(1)
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zode_node_protocol::NodeId;

    fn session() -> SessionLocator {
        SessionLocator::new(
            NodeId::parse("00000000-0000-0000-0000-000000000001").unwrap(),
            "s",
        )
    }

    #[test]
    fn opens_at_the_default_zoom_step() {
        let lightbox = LightboxState::new(session(), "image:1".into());
        assert_eq!(lightbox.zoom_percent(), 65);
    }

    #[test]
    fn zoom_steps_saturate_instead_of_wrapping() {
        let mut lightbox = LightboxState::new(session(), "image:1".into());
        for _ in 0..20 {
            lightbox.step_zoom(true);
        }
        assert_eq!(lightbox.zoom_percent(), 200);
        for _ in 0..20 {
            lightbox.step_zoom(false);
        }
        assert_eq!(lightbox.zoom_percent(), 25);
    }
}
