//! Services: named, type-checked, fiber-owned values with lazy factories.
//!
//! Services are stored centrally in the root state, keyed by
//! (isolation scope, name). They are removed automatically when their
//! providing fiber unloads, and lazy services are only constructed on first
//! access — an unused heavy dependency costs nothing.

use std::any::Any;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use serde_json::json;

use crate::context::{Context, RootInner};
use crate::error::CordisError;
use crate::fiber::{current_fiber_or_root, FiberId};
use crate::types::{Cleanup, Disposer};

pub(crate) type ServiceValue = Arc<dyn Any + Send + Sync>;
/// (isolation label, service name) — services in different scopes are
/// independent, so an isolated context can override a parent service
/// without affecting it.
pub(crate) type ScopeKey = (Option<Arc<str>>, Arc<str>);
pub(crate) type ServiceFactory =
    Box<dyn Fn(&Context) -> Result<ServiceValue, CordisError> + Send + Sync>;

pub(crate) enum Slot {
    Eager(ServiceValue),
    Lazy {
        factory: ServiceFactory,
        cell: OnceLock<Result<ServiceValue, Arc<CordisError>>>,
        resolved: AtomicBool,
    },
}

pub(crate) struct ServiceEntry {
    /// Fiber that provided this service (removed on its unload).
    pub owner: FiberId,
    /// Optional availability predicate evaluated in the reading context.
    pub check: Option<fn(&Context) -> bool>,
    pub slot: Mutex<Slot>,
    pub lazy: bool,
}

impl ServiceEntry {
    pub(crate) fn resolve(&self, ctx: &Context) -> Result<ServiceValue, CordisError> {
        let mut slot = self.slot.lock().unwrap();
        match &mut *slot {
            Slot::Eager(value) => Ok(value.clone()),
            Slot::Lazy {
                factory,
                cell,
                resolved,
            } => {
                if let Some(result) = cell.get() {
                    return result.clone().map_err(|error| error.as_ref().clone());
                }
                if !resolved.swap(true, Ordering::AcqRel) {
                    ctx.mark_lazy_initialized();
                }
                let result = factory(ctx).map_err(Arc::new);
                let _ = cell.set(result.clone());
                result.map_err(|error| error.as_ref().clone())
            }
        }
    }

    fn resolved(&self) -> bool {
        match &*self.slot.lock().unwrap() {
            Slot::Eager(_) => true,
            Slot::Lazy { resolved, .. } => resolved.load(Ordering::Acquire),
        }
    }
}

/// Remove a service entry owned by `owner` and notify dependents.
pub(crate) fn remove_entry(root: &Arc<RootInner>, key: &ScopeKey, owner: FiberId) -> bool {
    let removed = {
        let mut services = root.services.write().unwrap();
        let owned = matches!(services.get(key), Some(entry) if entry.owner == owner);
        if owned {
            services.remove(key)
        } else {
            None
        }
    };
    let Some(entry) = removed else {
        return false;
    };
    if entry.lazy && !entry.resolved() {
        root.lazy_uninitialized.fetch_sub(1, Ordering::SeqCst);
    }
    crate::registry::emit_internal(
        root,
        "internal/service",
        json!({ "name": key.1.as_ref(), "fiber": owner.0, "phase": "removed" }),
    );
    crate::registry::on_service_changed(root, &key.1);
    true
}

impl Context {
    /// Provide a service value under `name` (typed by the value's type).
    /// Returns a disposer that removes the service early; otherwise it is
    /// removed when the providing fiber unloads.
    pub fn provide<T: Any + Send + Sync>(
        &self,
        name: &str,
        value: T,
    ) -> Result<Disposer, CordisError> {
        self.install(name, Slot::Eager(Arc::new(value)), None, false)
    }

    /// Like `provide`, with an availability predicate evaluated in the
    /// reading context (a failing check hides the service).
    pub fn provide_checked<T: Any + Send + Sync>(
        &self,
        name: &str,
        value: T,
        check: fn(&Context) -> bool,
    ) -> Result<Disposer, CordisError> {
        self.install(name, Slot::Eager(Arc::new(value)), Some(check), false)
    }

    /// Provide a lazy service: the factory runs at most once, on first
    /// access. Never-accessed services are never allocated.
    pub fn provide_lazy<T: Any + Send + Sync, F>(
        &self,
        name: &str,
        factory: F,
    ) -> Result<Disposer, CordisError>
    where
        F: Fn(&Context) -> Result<Arc<T>, CordisError> + Send + Sync + 'static,
    {
        let factory =
            Box::new(move |ctx: &Context| factory(ctx).map(|value| value as ServiceValue));
        self.install(
            name,
            Slot::Lazy {
                factory,
                cell: OnceLock::new(),
                resolved: AtomicBool::new(false),
            },
            None,
            true,
        )
    }

    /// Resolve a typed service. Fails with `ServiceNotFound` when absent
    /// (or hidden by its check) and `ServiceTypeMismatch` when the stored
    /// type differs.
    pub fn use_service<T: Any + Send + Sync>(&self, name: &str) -> Result<Arc<T>, CordisError> {
        let entry = self.service_entry(name)?;
        let value = entry.resolve(self)?;
        value
            .downcast::<T>()
            .map_err(|_| CordisError::ServiceTypeMismatch(name.to_string()))
    }

    /// Resolve a service without a static type check.
    pub fn service(&self, name: &str) -> Result<ServiceValue, CordisError> {
        self.service_entry(name)?.resolve(self)
    }

    pub fn has_service(&self, name: &str) -> bool {
        self.service_entry(name).is_ok()
    }

    /// Remove a service immediately (scope-aware). Dependents reload.
    pub fn remove_service(&self, name: &str) -> bool {
        if self.check_alive().is_err() {
            return false;
        }
        let root = self.root_state();
        let key: ScopeKey = (self.isolation_label(name), Arc::from(name));
        let owner = match root.services.read().unwrap().get(&key) {
            Some(entry) => entry.owner,
            None => return false,
        };
        remove_entry(&root, &key, owner)
    }

    /// Sorted names of all provided services (for diagnostics).
    pub fn service_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .root_state()
            .services
            .read()
            .unwrap()
            .keys()
            .map(|(_, name)| name.to_string())
            .collect();
        names.sort();
        names
    }

    fn install(
        &self,
        name: &str,
        slot: Slot,
        check: Option<fn(&Context) -> bool>,
        lazy: bool,
    ) -> Result<Disposer, CordisError> {
        self.check_alive()?;
        let root = self.root_state();
        let budget = *root.budget.read().unwrap();
        let key: ScopeKey = (self.isolation_label(name), Arc::from(name));
        {
            let services = root.services.read().unwrap();
            if !services.contains_key(&key) && services.len() >= budget.max_services {
                return Err(CordisError::BudgetExceeded("max_services"));
            }
        }
        let fiber = current_fiber_or_root(self).ok_or(CordisError::InactiveEffect)?;
        let owner = fiber.id;
        let entry = Arc::new(ServiceEntry {
            owner,
            check,
            slot: Mutex::new(slot),
            lazy,
        });
        if lazy {
            root.lazy_uninitialized.fetch_add(1, Ordering::SeqCst);
        }
        let replaced = root.services.write().unwrap().insert(key.clone(), entry);
        if let Some(old) = &replaced {
            // The replaced entry is dropped: account for its lazy counter.
            if old.lazy && !old.resolved() {
                root.lazy_uninitialized.fetch_sub(1, Ordering::SeqCst);
            }
        }
        crate::registry::emit_internal(
            &root,
            "internal/service",
            json!({ "name": key.1.as_ref(), "fiber": owner.0, "phase": "provided" }),
        );
        if replaced.is_some() {
            // A replaced service reloads fibers that depend on it.
            crate::registry::on_service_changed(&root, name);
        } else {
            // A newly available service may satisfy pending inject fibers.
            crate::registry::recheck_pending(&root);
        }
        // The disposer is BOTH returned (for early removal) and registered
        // on the providing fiber, so the service is removed automatically
        // when the fiber unloads (Cordis effect semantics).
        let root = Arc::downgrade(&root);
        let disposer = Disposer::new(Cleanup::sync(move || {
            if let Some(root) = root.upgrade() {
                remove_entry(&root, &key, owner);
            }
        }));
        fiber.register_disposer(Arc::from(format!("service:{name}")), disposer.clone());
        Ok(disposer)
    }

    fn service_entry(&self, name: &str) -> Result<Arc<ServiceEntry>, CordisError> {
        self.check_alive()?;
        let root = self.root_state();
        let key: ScopeKey = (self.isolation_label(name), Arc::from(name));
        let entry = root
            .services
            .read()
            .unwrap()
            .get(&key)
            .cloned()
            .ok_or_else(|| CordisError::ServiceNotFound(name.to_string()))?;
        if let Some(check) = entry.check {
            if !check(self) {
                return Err(CordisError::ServiceNotFound(name.to_string()));
            }
        }
        Ok(entry)
    }
}
