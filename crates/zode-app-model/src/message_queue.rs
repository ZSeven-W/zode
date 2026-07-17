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
        if text.trim().is_empty() && attachments.is_empty() {
            return None;
        }
        let id = QueuedMessageId(self.next_id);
        self.next_id = self
            .next_id
            .checked_add(1)
            .expect("a session queue cannot allocate more than u64::MAX messages");
        self.items.push(QueuedMessage {
            id,
            text,
            attachments,
        });
        Some(id)
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
}
