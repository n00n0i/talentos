//! Sandbox tests against the real example plugin.
//!
//! Skips when the guest wasm has not been built:
//!   (cd plugins/bonus-calculator-thai && cargo build --release --target wasm32-wasip1)

use talentos_kernel::TenantId;
use talentos_wasm_host::{
    registry::example_plugin_bytes, Capability, CapabilitySet, InvokeOutcome, PluginError,
    PluginHost, PluginLimits, PluginSpec,
};
use uuid::Uuid;

fn tenant() -> TenantId {
    TenantId(Uuid::new_v4())
}

fn spec() -> Option<PluginSpec> {
    example_plugin_bytes().map(|wasm| PluginSpec {
        name: "bonus-calculator-thai".to_string(),
        wasm,
        requires: CapabilitySet::of([Capability::TenantContext]),
    })
}

fn host() -> PluginHost {
    PluginHost::new(PluginLimits::default())
}

fn invoke(host: &mut PluginHost, t: TenantId, input: serde_json::Value) -> InvokeOutcome {
    host.invoke(t, "bonus-calculator-thai", "calculate_bonus", input)
        .expect("invoke")
}

#[test]
fn full_year_bonus_is_base_times_months() {
    let Some(spec) = spec() else {
        eprintln!("SKIP: example plugin wasm not built");
        return;
    };
    let t = tenant();
    let mut h = host();
    h.register(t, spec, CapabilitySet::of([Capability::TenantContext]))
        .expect("register");

    let out = invoke(
        &mut h,
        t,
        serde_json::json!({"base_salary": "85000.00", "months": "1.0", "service_months": 24}),
    );
    assert_eq!(out.output["bonus"], "85000.00");
}

#[test]
fn under_a_year_of_service_gets_no_bonus() {
    let Some(spec) = spec() else {
        eprintln!("SKIP: example plugin wasm not built");
        return;
    };
    let t = tenant();
    let mut h = host();
    h.register(t, spec, CapabilitySet::of([Capability::TenantContext]))
        .expect("register");

    let out = invoke(
        &mut h,
        t,
        serde_json::json!({"base_salary": "30000", "months": "1.0", "service_months": 6}),
    );
    assert_eq!(out.output["bonus"], "0.00");
}

#[test]
fn registration_is_refused_without_the_granted_capability() {
    let Some(spec) = spec() else {
        eprintln!("SKIP: example plugin wasm not built");
        return;
    };
    let t = tenant();
    let mut h = host();
    let err = h.register(t, spec, CapabilitySet::empty()).unwrap_err();
    match err {
        PluginError::MissingCapability { plugin, capability } => {
            assert_eq!(plugin, "bonus-calculator-thai");
            assert_eq!(capability, "tenant_context");
        }
        other => panic!("expected MissingCapability, got {other:?}"),
    }
    assert!(
        h.list(t).is_empty(),
        "refused plugin must not be registered"
    );
}

#[test]
fn invocation_is_tenant_scoped() {
    let Some(spec_a) = spec() else {
        eprintln!("SKIP: example plugin wasm not built");
        return;
    };
    let Some(spec_b) = spec() else { return };
    let (t1, t2) = (tenant(), tenant());
    let mut h = host();
    h.register(t1, spec_a, CapabilitySet::of([Capability::TenantContext]))
        .expect("register t1");

    assert_eq!(h.list(t1), vec!["bonus-calculator-thai".to_string()]);
    assert!(h.list(t2).is_empty(), "plugins never leak across tenants");

    let err = h
        .invoke(
            t2,
            "bonus-calculator-thai",
            "calculate_bonus",
            serde_json::json!({}),
        )
        .unwrap_err();
    assert!(matches!(err, PluginError::NotFound(_)));

    h.register(t2, spec_b, CapabilitySet::of([Capability::TenantContext]))
        .expect("register t2");
    h.unregister(t2, "bonus-calculator-thai");
    assert!(h.list(t2).is_empty());
}

#[test]
fn bad_input_surfaces_a_plugin_error_not_a_panic() {
    let Some(spec) = spec() else {
        eprintln!("SKIP: example plugin wasm not built");
        return;
    };
    let t = tenant();
    let mut h = host();
    h.register(t, spec, CapabilitySet::of([Capability::TenantContext]))
        .expect("register");

    let err = h
        .invoke(
            t,
            "bonus-calculator-thai",
            "calculate_bonus",
            serde_json::json!({"base_salary": "not-a-number", "months": "1", "service_months": 12}),
        )
        .unwrap_err();
    assert!(matches!(err, PluginError::Invoke { .. }), "got {err:?}");
}

#[test]
fn non_wasm_bytes_are_rejected() {
    let mut h = host();
    let err = h
        .register(
            tenant(),
            PluginSpec {
                name: "evil".into(),
                wasm: b"#!/bin/sh\nrm -rf /".to_vec(),
                requires: CapabilitySet::empty(),
            },
            CapabilitySet::empty(),
        )
        .unwrap_err();
    assert!(matches!(err, PluginError::Invalid(_)));
}
