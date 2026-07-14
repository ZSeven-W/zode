use tokio::sync::mpsc;

use super::task_protocol::TaskClientFrame;

pub(crate) const TASK_CHANNEL_CAPACITY: usize = 64;

#[derive(Debug, Clone, PartialEq)]
pub struct TaskInbound {
    pub connection_id: u64,
    pub kind: TaskInboundKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TaskInboundKind {
    Request(TaskClientFrame),
    Disconnected,
}

pub type TaskReceiver = mpsc::Receiver<TaskInbound>;
pub(crate) type TaskSender = mpsc::Sender<TaskInbound>;

pub(crate) fn task_channel() -> (TaskSender, TaskReceiver) {
    mpsc::channel(TASK_CHANNEL_CAPACITY)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn inbound(id: u64) -> TaskInbound {
        TaskInbound {
            connection_id: id,
            kind: TaskInboundKind::Disconnected,
        }
    }

    #[tokio::test]
    async fn task_channel_is_bounded_and_backpressures_at_capacity() {
        let (sender, mut receiver) = task_channel();

        for id in 0..TASK_CHANNEL_CAPACITY as u64 {
            sender
                .try_send(inbound(id))
                .expect("capacity accepts the configured number of frames");
        }
        assert!(matches!(
            sender.try_send(inbound(999)),
            Err(mpsc::error::TrySendError::Full(_))
        ));

        let reserve = sender.reserve();
        tokio::pin!(reserve);
        assert!(
            tokio::time::timeout(Duration::from_millis(20), reserve.as_mut())
                .await
                .is_err()
        );

        assert_eq!(receiver.recv().await, Some(inbound(0)));
        let permit = tokio::time::timeout(Duration::from_secs(1), reserve.as_mut())
            .await
            .expect("freeing one slot releases a blocked producer")
            .expect("receiver remains open");
        permit.send(inbound(999));
    }
}
