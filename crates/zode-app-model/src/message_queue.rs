use std::fmt;

use crate::AttachmentMetadata;

/// Stable identity for one message while it is waiting in a session queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct QueuedMessageId(u64);

impl QueuedMessageId {
    pub const fn from_u64(value: u64) -> Self {
        Self(value)
    }

    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

impl fmt::Display for QueuedMessageId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Lightweight queue projection. Encoded image payloads stay in the controller.
#[derive(Debug, Clone, PartialEq)]
pub struct QueuedMessage {
    pub id: QueuedMessageId,
    pub text: String,
    pub attachments: Vec<AttachmentMetadata>,
    /// Set when this message reached the front of the queue through an
    /// explicit priority action (`enqueue_front` or `move_to_front`), so the
    /// UI can badge it. A message that simply advances to the front because
    /// earlier entries were dispatched keeps this `false`.
    pub prioritized: bool,
}

/// Ordered pending-message previews for exactly one session.
#[derive(Debug, Clone, PartialEq)]
pub struct MessageQueueState {
    pub items: Vec<QueuedMessage>,
    next_id: u64,
}

impl Default for MessageQueueState {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            next_id: 1,
        }
    }
}

impl MessageQueueState {
    pub fn enqueue(
        &mut self,
        text: String,
        attachments: Vec<AttachmentMetadata>,
    ) -> Option<QueuedMessageId> {
        let id = self.allocate(&text, &attachments)?;
        self.items.push(QueuedMessage {
            id,
            text,
            attachments,
            prioritized: false,
        });
        Some(id)
    }

    /// Inserts a message at the head of the queue, ahead of everything
    /// already waiting. Used for priority send while a turn is running.
    pub fn enqueue_front(
        &mut self,
        text: String,
        attachments: Vec<AttachmentMetadata>,
    ) -> Option<QueuedMessageId> {
        let id = self.allocate(&text, &attachments)?;
        self.items.insert(
            0,
            QueuedMessage {
                id,
                text,
                attachments,
                prioritized: true,
            },
        );
        Some(id)
    }

    fn allocate(
        &mut self,
        text: &str,
        attachments: &[AttachmentMetadata],
    ) -> Option<QueuedMessageId> {
        if text.trim().is_empty() && attachments.is_empty() {
            return None;
        }
        let id = QueuedMessageId(self.next_id);
        self.next_id = self
            .next_id
            .checked_add(1)
            .expect("a session queue cannot allocate more than u64::MAX messages");
        Some(id)
    }

    /// Moves a message to the head of the queue. Returns `false` (no-op,
    /// state left untouched) for an unknown id or a message already at the
    /// front, so callers never dirty the queue without a real change.
    pub fn move_to_front(&mut self, id: QueuedMessageId) -> bool {
        let Some(index) = self.items.iter().position(|message| message.id == id) else {
            return false;
        };
        if index == 0 {
            return false;
        }
        let mut message = self.items.remove(index);
        message.prioritized = true;
        self.items.insert(0, message);
        true
    }

    /// Swaps a message with its predecessor. Returns `false` for an unknown
    /// id or a message already at the front.
    pub fn move_up(&mut self, id: QueuedMessageId) -> bool {
        let Some(index) = self.items.iter().position(|message| message.id == id) else {
            return false;
        };
        if index == 0 {
            return false;
        }
        self.items.swap(index, index - 1);
        true
    }

    /// Swaps a message with its successor. Returns `false` for an unknown id
    /// or a message already at the back.
    pub fn move_down(&mut self, id: QueuedMessageId) -> bool {
        let Some(index) = self.items.iter().position(|message| message.id == id) else {
            return false;
        };
        if index + 1 >= self.items.len() {
            return false;
        }
        self.items.swap(index, index + 1);
        true
    }

    pub fn edit_text(&mut self, id: QueuedMessageId, text: String) -> bool {
        let Some(message) = self.items.iter_mut().find(|message| message.id == id) else {
            return false;
        };
        if text.trim().is_empty() && message.attachments.is_empty() {
            return false;
        }
        message.text = text;
        true
    }

    /// Returns the next dispatchable preview without removing it. The item
    /// currently being edited is never dispatchable.
    pub fn peek_next(&self, editing_id: Option<QueuedMessageId>) -> Option<&QueuedMessage> {
        self.items
            .first()
            .filter(|message| Some(message.id) != editing_id)
    }

    /// Removes exactly one accepted message. Controllers call this only after
    /// the command channel accepts the corresponding payload dispatch.
    pub fn remove_exact(&mut self, id: QueuedMessageId) -> Option<QueuedMessage> {
        let index = self.items.iter().position(|message| message.id == id)?;
        Some(self.items.remove(index))
    }

    pub fn clear(&mut self) -> bool {
        if self.items.is_empty() {
            return false;
        }
        self.items.clear();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::{MessageQueueState, QueuedMessageId};
    use crate::AttachmentMetadata;

    #[test]
    fn image_only_preview_is_a_valid_queue_entry() {
        let mut queue = MessageQueueState::default();
        let id = queue
            .enqueue(
                String::new(),
                vec![AttachmentMetadata {
                    id: "image-1".into(),
                    path: None,
                    display_name: "reference.png".into(),
                    media_type: "image/png".into(),
                    byte_len: 3,
                    width: None,
                    height: None,
                }],
            )
            .unwrap();

        assert_eq!(id.as_u64(), 1);
        assert_eq!(queue.items[0].attachments.len(), 1);
    }

    #[test]
    fn peek_skips_the_message_being_edited_and_remove_is_exact() {
        let mut queue = MessageQueueState::default();
        let first = queue.enqueue("first".into(), Vec::new()).unwrap();
        let second = queue.enqueue("second".into(), Vec::new()).unwrap();

        assert_eq!(QueuedMessageId::from_u64(7).as_u64(), 7);
        assert_eq!(queue.peek_next(None).map(|message| message.id), Some(first));
        assert_eq!(queue.peek_next(Some(first)), None);
        assert_eq!(
            queue.remove_exact(second).map(|message| message.id),
            Some(second)
        );
        assert_eq!(queue.items.len(), 1);
    }

    #[test]
    fn enqueue_front_inserts_ahead_of_existing_items_and_marks_prioritized() {
        let mut queue = MessageQueueState::default();
        let first = queue.enqueue("first".into(), Vec::new()).unwrap();
        let jumped = queue.enqueue_front("jumped".into(), Vec::new()).unwrap();

        assert_eq!(queue.items[0].id, jumped);
        assert!(queue.items[0].prioritized);
        assert_eq!(queue.items[1].id, first);
        assert!(!queue.items[1].prioritized);
        // Ids keep allocating from the same counter regardless of position.
        assert_eq!(jumped.as_u64(), 2);
    }

    #[test]
    fn enqueue_front_rejects_empty_content_like_enqueue() {
        let mut queue = MessageQueueState::default();
        assert!(queue.enqueue_front(String::new(), Vec::new()).is_none());
        assert!(queue.items.is_empty());
    }

    #[test]
    fn move_to_front_reorders_and_marks_prioritized_but_is_a_boundary_no_op_at_the_front() {
        let mut queue = MessageQueueState::default();
        let first = queue.enqueue("first".into(), Vec::new()).unwrap();
        let second = queue.enqueue("second".into(), Vec::new()).unwrap();
        let third = queue.enqueue("third".into(), Vec::new()).unwrap();

        assert!(queue.move_to_front(third));
        assert_eq!(
            queue.items.iter().map(|item| item.id).collect::<Vec<_>>(),
            vec![third, first, second]
        );
        assert!(queue.items[0].prioritized);

        // Already at the front: no-op, does not re-dirty or toggle anything.
        assert!(!queue.move_to_front(third));
        assert_eq!(
            queue.items.iter().map(|item| item.id).collect::<Vec<_>>(),
            vec![third, first, second]
        );
    }

    #[test]
    fn move_to_front_on_an_unknown_id_is_ignored() {
        let mut queue = MessageQueueState::default();
        queue.enqueue("only".into(), Vec::new()).unwrap();

        assert!(!queue.move_to_front(QueuedMessageId::from_u64(999)));
        assert_eq!(queue.items.len(), 1);
    }

    #[test]
    fn move_up_swaps_with_predecessor_and_is_a_boundary_no_op_at_the_front() {
        let mut queue = MessageQueueState::default();
        let first = queue.enqueue("first".into(), Vec::new()).unwrap();
        let second = queue.enqueue("second".into(), Vec::new()).unwrap();

        assert!(queue.move_up(second));
        assert_eq!(
            queue.items.iter().map(|item| item.id).collect::<Vec<_>>(),
            vec![second, first]
        );
        // move_up never marks the item prioritized (only move_to_front does).
        assert!(!queue.items[0].prioritized);

        assert!(!queue.move_up(second));
        assert_eq!(
            queue.items.iter().map(|item| item.id).collect::<Vec<_>>(),
            vec![second, first]
        );
        assert!(!queue.move_up(QueuedMessageId::from_u64(999)));
    }

    #[test]
    fn move_down_swaps_with_successor_and_is_a_boundary_no_op_at_the_back() {
        let mut queue = MessageQueueState::default();
        let first = queue.enqueue("first".into(), Vec::new()).unwrap();
        let second = queue.enqueue("second".into(), Vec::new()).unwrap();

        assert!(queue.move_down(first));
        assert_eq!(
            queue.items.iter().map(|item| item.id).collect::<Vec<_>>(),
            vec![second, first]
        );

        assert!(!queue.move_down(first));
        assert_eq!(
            queue.items.iter().map(|item| item.id).collect::<Vec<_>>(),
            vec![second, first]
        );
        assert!(!queue.move_down(QueuedMessageId::from_u64(999)));
    }

    #[test]
    fn reorder_operations_on_an_empty_or_single_item_queue_are_always_no_ops() {
        let mut empty = MessageQueueState::default();
        let missing = QueuedMessageId::from_u64(1);
        assert!(!empty.move_to_front(missing));
        assert!(!empty.move_up(missing));
        assert!(!empty.move_down(missing));

        let mut single = MessageQueueState::default();
        let only = single.enqueue("only".into(), Vec::new()).unwrap();
        assert!(!single.move_to_front(only));
        assert!(!single.move_up(only));
        assert!(!single.move_down(only));
    }
}
