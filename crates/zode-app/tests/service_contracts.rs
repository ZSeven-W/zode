use std::sync::Arc;

use zode_app::services::{
    ExternalOpenService, FileService, NotificationService, WindowService, WorkspaceService,
};

fn accept_remote_ready_services(
    _: Arc<dyn WindowService>,
    _: Arc<dyn WorkspaceService>,
    _: Arc<dyn FileService>,
    _: Arc<dyn NotificationService>,
    _: Arc<dyn ExternalOpenService>,
) {
}

#[test]
fn platform_services_are_object_safe() {
    let _ = accept_remote_ready_services;
}
