use super::{NotificationService, ServiceError};

#[derive(Default)]
pub struct LocalNotificationService;

impl NotificationService for LocalNotificationService {
    fn notify(&self, title: &str, body: &str) -> Result<(), ServiceError> {
        notify_rust::Notification::new()
            .summary(title)
            .body(body)
            .show()
            .map(|_| ())
            .map_err(|error| ServiceError::Platform(error.to_string()))
    }
}
