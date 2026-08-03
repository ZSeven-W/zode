use std::{collections::BTreeMap, sync::Arc};

use tokio::sync::mpsc;
use zode_app_model::{
    integration_catalog, EnvironmentSnapshot, LoadState, PluginDetailMode, PluginUpdateState,
    PreviewKind, PreviewState, PreviewTarget, PullRequestStatus, ZodeAppState,
};
use zode_app_runtime::{
    git_status::{fetch_pull_request_status, SystemCommandRunner},
    workspace_uri_to_path,
};
use zode_node_protocol::{
    AgentEndpoint, AgentQuery, AgentSnapshot, DiffSnapshot, InstalledPluginSummary,
    IntegrationRegistrySnapshot, PluginTrustReview, PluginUpdateCheck, SessionLocator,
    WorkspaceUri,
};

use crate::{
    services::{FileService, LocalFileService},
    window_state::AppWake,
};

const MAX_PREVIEW_BYTES: u64 = 1024 * 1024;

/// One presentation-only snapshot request that must not block the window loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PresentationQuery {
    Environment {
        session: SessionLocator,
        workspace_uri: WorkspaceUri,
    },
    Diff {
        session: SessionLocator,
    },
    DocumentPreview {
        session: SessionLocator,
        target: PreviewTarget,
    },
    Integrations {
        workspace_uri: WorkspaceUri,
    },
    InstalledPlugins,
    PluginTrustReview {
        plugin_id: String,
    },
    /// Git-fetches the plugin's pinned ref and compares it with the local
    /// checkout. Slow, so it rides the same background pump as every other
    /// presentation query rather than blocking the window loop.
    PluginUpdateCheck {
        plugin_id: String,
    },
    PullRequest {
        session: SessionLocator,
        workspace_uri: WorkspaceUri,
    },
}

impl PresentationQuery {
    fn key(&self) -> QueryKey {
        match self {
            Self::Environment { session, .. } => QueryKey::Environment(session.clone()),
            Self::Diff { session } => QueryKey::Diff(session.clone()),
            Self::DocumentPreview { session, .. } => QueryKey::DocumentPreview(session.clone()),
            Self::Integrations { .. } => QueryKey::Integrations,
            Self::InstalledPlugins => QueryKey::InstalledPlugins,
            Self::PluginTrustReview { .. } => QueryKey::PluginTrustReview,
            Self::PluginUpdateCheck { .. } => QueryKey::PluginUpdateCheck,
            Self::PullRequest { session, .. } => QueryKey::PullRequest(session.clone()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum QueryKey {
    Environment(SessionLocator),
    Diff(SessionLocator),
    DocumentPreview(SessionLocator),
    Integrations,
    InstalledPlugins,
    PluginTrustReview,
    PluginUpdateCheck,
    PullRequest(SessionLocator),
}

struct PendingQuery {
    generation: u64,
    latest: PresentationQuery,
}

struct WorkItem {
    generation: u64,
    query: PresentationQuery,
}

enum BridgeItem {
    Diff {
        generation: u64,
        session: SessionLocator,
        result: Result<DiffSnapshot, String>,
    },
    Environment {
        generation: u64,
        session: SessionLocator,
        workspace_uri: WorkspaceUri,
        result: Result<EnvironmentSnapshot, String>,
    },
    DocumentPreview {
        generation: u64,
        session: SessionLocator,
        target: PreviewTarget,
        result: Result<LoadedPreview, String>,
    },
    Integrations {
        generation: u64,
        workspace_uri: WorkspaceUri,
        result: Result<IntegrationRegistrySnapshot, String>,
    },
    InstalledPlugins {
        generation: u64,
        result: Result<Vec<InstalledPluginSummary>, String>,
    },
    PluginTrustReview {
        generation: u64,
        plugin_id: String,
        result: Result<PluginTrustReview, String>,
    },
    PluginUpdateCheck {
        generation: u64,
        plugin_id: String,
        result: Result<PluginUpdateCheck, String>,
    },
    PullRequest {
        generation: u64,
        session: SessionLocator,
        status: PullRequestStatus,
    },
}

struct LoadedPreview {
    title: String,
    content: String,
    kind: PreviewKind,
}

impl BridgeItem {
    fn key(&self) -> QueryKey {
        match self {
            Self::Environment { session, .. } => QueryKey::Environment(session.clone()),
            Self::Diff { session, .. } => QueryKey::Diff(session.clone()),
            Self::DocumentPreview { session, .. } => QueryKey::DocumentPreview(session.clone()),
            Self::Integrations { .. } => QueryKey::Integrations,
            Self::InstalledPlugins { .. } => QueryKey::InstalledPlugins,
            Self::PluginTrustReview { .. } => QueryKey::PluginTrustReview,
            Self::PluginUpdateCheck { .. } => QueryKey::PluginUpdateCheck,
            Self::PullRequest { session, .. } => QueryKey::PullRequest(session.clone()),
        }
    }

    fn generation(&self) -> u64 {
        match self {
            Self::Environment { generation, .. }
            | Self::Diff { generation, .. }
            | Self::DocumentPreview { generation, .. }
            | Self::Integrations { generation, .. }
            | Self::InstalledPlugins { generation, .. }
            | Self::PluginTrustReview { generation, .. }
            | Self::PluginUpdateCheck { generation, .. }
            | Self::PullRequest { generation, .. } => *generation,
        }
    }
}

/// Asynchronous query pump for presentation data that is not carried by events.
pub struct PresentationQueryBridge {
    requests: mpsc::UnboundedSender<WorkItem>,
    results: mpsc::UnboundedReceiver<BridgeItem>,
    pending: BTreeMap<QueryKey, PendingQuery>,
}

impl PresentationQueryBridge {
    /// Starts the query worker and wakes the desktop event loop on completion.
    pub fn spawn(
        endpoint: Arc<dyn AgentEndpoint>,
        proxy: winit::event_loop::EventLoopProxy<AppWake>,
    ) -> Self {
        Self::spawn_with_services(endpoint, Arc::new(LocalFileService), move || {
            let _ = proxy.send_event(AppWake::Redraw);
        })
    }

    /// Starts the query worker with an injected wake callback.
    pub fn spawn_with_wake(
        endpoint: Arc<dyn AgentEndpoint>,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        Self::spawn_with_services(endpoint, Arc::new(LocalFileService), wake)
    }

    pub(crate) fn spawn_with_services(
        endpoint: Arc<dyn AgentEndpoint>,
        files: Arc<dyn FileService>,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        let (request_sender, mut requests) = mpsc::unbounded_channel::<WorkItem>();
        let (result_sender, results) = mpsc::unbounded_channel::<BridgeItem>();
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(wake);
        tokio::spawn(async move {
            while let Some(work) = requests.recv().await {
                let endpoint = Arc::clone(&endpoint);
                let files = Arc::clone(&files);
                let result_sender = result_sender.clone();
                let wake = Arc::clone(&wake);
                tokio::spawn(async move {
                    let item = execute_work(endpoint, files, work).await;
                    if result_sender.send(item).is_ok() {
                        wake();
                    }
                });
            }
        });
        Self {
            requests: request_sender,
            results,
            pending: BTreeMap::new(),
        }
    }

    /// Queues a request and projects its loading state synchronously.
    pub fn request(
        &mut self,
        state: &mut ZodeAppState,
        request: PresentationQuery,
    ) -> Result<(), PresentationQuery> {
        let key = request.key();
        if let Some(pending) = self.pending.get_mut(&key) {
            pending.generation = pending.generation.saturating_add(1);
            pending.latest = request.clone();
            mark_loading(state, request);
            return Ok(());
        }
        let work = WorkItem {
            generation: 1,
            query: request.clone(),
        };
        self.pending.insert(
            key.clone(),
            PendingQuery {
                generation: 1,
                latest: request.clone(),
            },
        );
        if let Err(error) = self.requests.send(work) {
            self.pending.remove(&key);
            return Err(error.0.query);
        }
        mark_loading(state, request);
        Ok(())
    }

    /// Applies all completed query results on the window thread.
    pub fn drain_into(&mut self, state: &mut ZodeAppState) -> usize {
        let mut applied = 0;
        while let Ok(item) = self.results.try_recv() {
            let key = item.key();
            let generation = item.generation();
            let Some(pending) = self.pending.get(&key) else {
                continue;
            };
            if generation != pending.generation {
                let work = WorkItem {
                    generation: pending.generation,
                    query: pending.latest.clone(),
                };
                if self.requests.send(work).is_err() {
                    self.pending.remove(&key);
                }
                continue;
            }
            self.pending.remove(&key);
            match item {
                BridgeItem::Environment {
                    session,
                    workspace_uri,
                    result,
                    ..
                } => {
                    let workspace_is_current = state
                        .threads
                        .iter()
                        .find(|thread| thread.session == session)
                        .is_none_or(|thread| thread.workspace_uri == workspace_uri);
                    let Some(presentation) = state.presentation.sessions.get_mut(&session) else {
                        continue;
                    };
                    if !workspace_is_current {
                        presentation.context = LoadState::Idle;
                        continue;
                    }
                    presentation.context = match result {
                        Ok(snapshot) => LoadState::Ready(snapshot),
                        Err(message) => LoadState::Failed(message),
                    };
                    applied += 1;
                }
                BridgeItem::Diff {
                    session, result, ..
                } => {
                    let Some(presentation) = state.presentation.sessions.get_mut(&session) else {
                        continue;
                    };
                    match result {
                        Ok(snapshot) => {
                            presentation.diff.dirty = false;
                            presentation.diff.load = LoadState::Ready(snapshot);
                            if state.current_session.as_ref() == Some(&session) {
                                state.review.dirty = false;
                            }
                        }
                        Err(message) => presentation.diff.load = LoadState::Failed(message),
                    }
                    applied += 1;
                }
                BridgeItem::DocumentPreview {
                    session,
                    target,
                    result,
                    ..
                } => {
                    let session_is_current = state.transcripts.contains_key(&session)
                        && state
                            .threads
                            .iter()
                            .find(|thread| thread.session == session)
                            .is_some_and(|thread| {
                                thread.workspace_uri == target.workspace_uri
                                    && state.available_workspace(&thread.workspace_uri)
                            });
                    let Some(presentation) = state.presentation.sessions.get_mut(&session) else {
                        continue;
                    };
                    let target_is_current = presentation.preview.target() == Some(&target);
                    if !session_is_current || !target_is_current {
                        if target_is_current {
                            presentation.preview = PreviewState::Idle;
                        }
                        continue;
                    }
                    presentation.preview = match result {
                        Ok(loaded) => PreviewState::Ready {
                            target,
                            title: loaded.title,
                            content: loaded.content,
                            kind: loaded.kind,
                        },
                        Err(message) => PreviewState::Failed { target, message },
                    };
                    applied += 1;
                }
                BridgeItem::Integrations {
                    workspace_uri,
                    result,
                    ..
                } => {
                    if state.active_available_workspace() != Some(&workspace_uri) {
                        continue;
                    }
                    state.presentation.integrations = match result {
                        Ok(snapshot) if snapshot.workspace_uri == workspace_uri => {
                            LoadState::Ready(integration_catalog(snapshot))
                        }
                        Ok(_) => LoadState::Failed(
                            "the endpoint returned integrations for the wrong workspace".into(),
                        ),
                        Err(message) => LoadState::Failed(message),
                    };
                    applied += 1;
                }
                BridgeItem::InstalledPlugins { result, .. } => {
                    state.presentation.installed_plugins = match result {
                        Ok(plugins) => LoadState::Ready(plugins),
                        Err(message) => LoadState::Failed(message),
                    };
                    applied += 1;
                }
                BridgeItem::PluginTrustReview {
                    plugin_id, result, ..
                } => {
                    let showing_review = state
                        .presentation
                        .plugin_detail
                        .as_mut()
                        .filter(|detail| detail.plugin_id == plugin_id)
                        .and_then(|detail| match &mut detail.mode {
                            PluginDetailMode::TrustReview { review, .. } => Some(review),
                            _ => None,
                        });
                    if let Some(review) = showing_review {
                        *review = match result {
                            Ok(payload) => LoadState::Ready(payload),
                            Err(message) => LoadState::Failed(message),
                        };
                        applied += 1;
                    }
                }
                BridgeItem::PluginUpdateCheck {
                    plugin_id, result, ..
                } => {
                    // Only land the answer on the overlay that asked for it
                    // and is still waiting - the user may have closed it,
                    // opened another plugin, or pressed 更新 in the meantime.
                    let waiting = state.presentation.plugin_detail.as_mut().filter(|detail| {
                        detail.plugin_id == plugin_id
                            && detail.update == PluginUpdateState::Checking
                    });
                    if let Some(detail) = waiting {
                        detail.update = match result {
                            Ok(check) => match check.available {
                                Some(available) => PluginUpdateState::Available(available),
                                None => PluginUpdateState::UpToDate,
                            },
                            Err(message) => PluginUpdateState::CheckFailed(message),
                        };
                        applied += 1;
                    }
                }
                BridgeItem::PullRequest {
                    session, status, ..
                } => {
                    let Some(presentation) = state.presentation.sessions.get_mut(&session) else {
                        continue;
                    };
                    presentation.pull_request.dirty = false;
                    presentation.pull_request.load = LoadState::Ready(status);
                    applied += 1;
                }
            }
        }
        applied
    }
}

async fn execute_work(
    endpoint: Arc<dyn AgentEndpoint>,
    files: Arc<dyn FileService>,
    work: WorkItem,
) -> BridgeItem {
    let generation = work.generation;
    match work.query {
        PresentationQuery::Environment {
            session,
            workspace_uri,
        } => {
            let result = load_environment(workspace_uri.clone()).await;
            BridgeItem::Environment {
                generation,
                session,
                workspace_uri,
                result,
            }
        }
        PresentationQuery::Diff { session } => {
            let result = match endpoint
                .query(AgentQuery::Diff {
                    session: session.clone(),
                })
                .await
            {
                Ok(AgentSnapshot::Diff(snapshot)) if snapshot.session == session => Ok(snapshot),
                Ok(AgentSnapshot::Diff(_)) => {
                    Err("the endpoint returned a diff for the wrong session".into())
                }
                Ok(_) => Err("the endpoint returned the wrong diff snapshot".into()),
                Err(error) => Err(error.to_string()),
            };
            BridgeItem::Diff {
                generation,
                session,
                result,
            }
        }
        PresentationQuery::DocumentPreview { session, target } => {
            let result = load_document(files.as_ref(), &target).await;
            BridgeItem::DocumentPreview {
                generation,
                session,
                target,
                result,
            }
        }
        PresentationQuery::Integrations { workspace_uri } => {
            let result = match endpoint
                .query(AgentQuery::Integrations {
                    workspace_uri: workspace_uri.clone(),
                })
                .await
            {
                Ok(AgentSnapshot::Integrations(snapshot)) => Ok(snapshot),
                Ok(_) => Err("the endpoint returned the wrong integrations snapshot".into()),
                Err(error) => Err(error.to_string()),
            };
            BridgeItem::Integrations {
                generation,
                workspace_uri,
                result,
            }
        }
        PresentationQuery::InstalledPlugins => {
            let result = match endpoint.query(AgentQuery::InstalledPlugins).await {
                Ok(AgentSnapshot::InstalledPlugins(plugins)) => Ok(plugins),
                Ok(_) => Err("the endpoint returned the wrong installed-plugins snapshot".into()),
                Err(error) => Err(error.to_string()),
            };
            BridgeItem::InstalledPlugins { generation, result }
        }
        PresentationQuery::PluginTrustReview { plugin_id } => {
            let result = match endpoint
                .query(AgentQuery::PluginTrustReview {
                    plugin_id: plugin_id.clone(),
                })
                .await
            {
                Ok(AgentSnapshot::PluginTrustReview(review)) => Ok(review),
                Ok(_) => Err("the endpoint returned the wrong trust-review snapshot".into()),
                Err(error) => Err(error.to_string()),
            };
            BridgeItem::PluginTrustReview {
                generation,
                plugin_id,
                result,
            }
        }
        PresentationQuery::PluginUpdateCheck { plugin_id } => {
            let result = match endpoint
                .query(AgentQuery::PluginUpdateCheck {
                    plugin_id: plugin_id.clone(),
                })
                .await
            {
                Ok(AgentSnapshot::PluginUpdateCheck(check)) if check.plugin_id == plugin_id => {
                    Ok(check)
                }
                Ok(AgentSnapshot::PluginUpdateCheck(_)) => {
                    Err("the endpoint returned an update check for the wrong plugin".into())
                }
                Ok(_) => Err("the endpoint returned the wrong update-check snapshot".into()),
                Err(error) => Err(error.to_string()),
            };
            BridgeItem::PluginUpdateCheck {
                generation,
                plugin_id,
                result,
            }
        }
        PresentationQuery::PullRequest {
            session,
            workspace_uri,
        } => {
            let status = fetch_pull_request_status_for(workspace_uri).await;
            BridgeItem::PullRequest {
                generation,
                session,
                status,
            }
        }
    }
}

/// Resolves `workspace_uri` to a local path and runs the `gh`/`git`-backed
/// PR status provider against it - see `zode_app_runtime::git_status`.
/// Non-local workspaces (no `file://` scheme) have no local repository to
/// inspect a remote for, so they read as `NoRemote` without shelling out.
async fn fetch_pull_request_status_for(workspace_uri: WorkspaceUri) -> PullRequestStatus {
    if !workspace_uri.as_str().starts_with("file://") {
        return PullRequestStatus::NoRemote;
    }
    let Ok(path) = workspace_uri_to_path(&workspace_uri) else {
        return PullRequestStatus::NoRemote;
    };
    fetch_pull_request_status(&SystemCommandRunner, &path).await
}

fn mark_loading(state: &mut ZodeAppState, request: PresentationQuery) {
    match request {
        PresentationQuery::Environment { session, .. } => {
            state
                .presentation
                .sessions
                .entry(session)
                .or_default()
                .context = LoadState::Loading;
        }
        PresentationQuery::Diff { session } => {
            state
                .presentation
                .sessions
                .entry(session)
                .or_default()
                .diff
                .load = LoadState::Loading;
        }
        PresentationQuery::DocumentPreview { session, target } => {
            state
                .presentation
                .sessions
                .entry(session)
                .or_default()
                .preview = PreviewState::Loading { target };
        }
        PresentationQuery::Integrations { .. } => {
            state.presentation.integrations = LoadState::Loading;
        }
        PresentationQuery::InstalledPlugins => {
            state.presentation.installed_plugins = LoadState::Loading;
        }
        PresentationQuery::PluginTrustReview { plugin_id } => {
            if let Some(detail) = state
                .presentation
                .plugin_detail
                .as_mut()
                .filter(|detail| detail.plugin_id == plugin_id)
            {
                if let PluginDetailMode::TrustReview { review, .. } = &mut detail.mode {
                    *review = LoadState::Loading;
                }
            }
        }
        PresentationQuery::PluginUpdateCheck { plugin_id } => {
            // `AppCommand::CheckPluginUpdate` already parked the overlay in
            // `Checking`; this only covers a query enqueued from anywhere
            // else, and never overwrites an apply that started meanwhile.
            if let Some(detail) = state
                .presentation
                .plugin_detail
                .as_mut()
                .filter(|detail| detail.plugin_id == plugin_id && !detail.update.busy())
            {
                detail.update = PluginUpdateState::Checking;
            }
        }
        PresentationQuery::PullRequest { session, .. } => {
            state
                .presentation
                .sessions
                .entry(session)
                .or_default()
                .pull_request
                .load = LoadState::Loading;
        }
    }
}

async fn load_document(
    files: &dyn FileService,
    target: &PreviewTarget,
) -> Result<LoadedPreview, String> {
    let bytes = files
        .read_bounded(
            &target.workspace_uri,
            &target.relative_path,
            MAX_PREVIEW_BYTES,
        )
        .await
        .map_err(|error| error.to_string())?;
    let content = String::from_utf8(bytes)
        .map_err(|_| "the document is not valid UTF-8 and cannot be previewed".to_owned())?;
    if content.contains('\0') {
        return Err("the document appears to be binary and cannot be previewed".into());
    }
    let path = std::path::Path::new(&target.relative_path);
    let title = path
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or(&target.relative_path)
        .to_owned();
    let kind = match path
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("md" | "markdown") => PreviewKind::Markdown,
        _ => PreviewKind::PlainText,
    };
    Ok(LoadedPreview {
        title,
        content,
        kind,
    })
}

async fn load_environment(workspace_uri: WorkspaceUri) -> Result<EnvironmentSnapshot, String> {
    let branch = if workspace_uri.as_str().starts_with("file://") {
        let path = workspace_uri_to_path(&workspace_uri).map_err(|error| error.to_string())?;
        tokio::task::spawn_blocking(move || zode_core::instructions::detect_git_branch(&path))
            .await
            .map_err(|error| format!("git branch task failed: {error}"))?
    } else {
        None
    };
    Ok(EnvironmentSnapshot {
        workspace_uri,
        branch,
        background_processes: Vec::new(),
        sources: Vec::new(),
    })
}

#[cfg(test)]
#[path = "presentation_bridge_tests.rs"]
mod presentation_bridge_tests;
