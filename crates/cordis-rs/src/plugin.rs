//! Plugin trait and function-style plugin adapter.

use std::any::TypeId;
use std::future::Future;
use std::sync::Arc;

use async_trait::async_trait;
use futures_util::future::BoxFuture;
use serde_json::Value;

use crate::context::Context;
use crate::error::CordisError;

/// A reusable set of context operations — the Rust analogue of a Cordis
/// plugin. Function-style plugins can be built with `plugin_fn`.
#[async_trait]
pub trait Plugin: Send + Sync + 'static {
    /// Display name used for fiber diagnostics and logger names.
    fn name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    /// Services the plugin requires; it only loads while all are provided,
    /// and reloads when any of them changes.
    fn inject(&self) -> &'static [&'static str] {
        &[]
    }

    /// Service names the plugin provides (metadata for loaders and
    /// introspection; the actual registration happens via `ctx.provide`).
    fn provide(&self) -> &'static [&'static str] {
        &[]
    }

    /// Validate raw config before the plugin starts.
    fn validate(&self, _config: &Value) -> Result<(), CordisError> {
        Ok(())
    }

    /// Identity of the plugin runtime (used to group fibers per plugin).
    fn runtime_type(&self) -> TypeId {
        TypeId::of::<Self>()
    }

    /// Optional content identity. Plugins whose code differs per instance
    /// (e.g. agent-generated JS source) return a hash of their content so
    /// gene dedupe distinguishes instances that share a wrapper type.
    fn content_id(&self) -> Option<String> {
        None
    }

    /// Run the plugin body in its own fiber context. Effects, services, and
    /// listeners registered here are owned by the fiber and torn down when
    /// it unloads.
    async fn apply(&self, ctx: Context, config: Arc<Value>) -> Result<(), CordisError>;
}

/// Result type returned by plugin bodies.
pub type PluginResult = Result<(), CordisError>;

/// A plugin defined by a closure `(ctx, config) -> impl Future`.
pub struct FunctionPlugin<F> {
    name: &'static str,
    inject: &'static [&'static str],
    provide: &'static [&'static str],
    callback: F,
}

impl<F> FunctionPlugin<F>
where
    F: Fn(Context, Arc<Value>) -> BoxFuture<'static, PluginResult> + Send + Sync + 'static,
{
    pub fn new(name: &'static str, callback: F) -> Self {
        FunctionPlugin {
            name,
            inject: &[],
            provide: &[],
            callback,
        }
    }

    pub fn with_inject(mut self, inject: &'static [&'static str]) -> Self {
        self.inject = inject;
        self
    }

    pub fn with_provide(mut self, provide: &'static [&'static str]) -> Self {
        self.provide = provide;
        self
    }
}

#[async_trait]
impl<F> Plugin for FunctionPlugin<F>
where
    F: Fn(Context, Arc<Value>) -> BoxFuture<'static, PluginResult> + Send + Sync + 'static,
{
    fn name(&self) -> &'static str {
        self.name
    }

    fn inject(&self) -> &'static [&'static str] {
        self.inject
    }

    fn provide(&self) -> &'static [&'static str] {
        self.provide
    }

    async fn apply(&self, ctx: Context, config: Arc<Value>) -> PluginResult {
        (self.callback)(ctx, config).await
    }
}

/// Build a function-style plugin from an async closure.
pub fn plugin_fn<F, Fut>(
    name: &'static str,
    f: F,
) -> FunctionPlugin<
    impl Fn(Context, Arc<Value>) -> BoxFuture<'static, PluginResult> + Send + Sync + 'static,
>
where
    F: Fn(Context, Arc<Value>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = PluginResult> + Send + 'static,
{
    FunctionPlugin::new(name, move |ctx, config| Box::pin(f(ctx, config)))
}
