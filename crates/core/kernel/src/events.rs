use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::tenant::TenantId;

/// Discriminant for event routing in Kafka topics and Temporal signals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    // Employee lifecycle
    EmployeeCreated,
    EmployeeUpdated,
    EmployeeTerminated,
    // Payroll
    PayrollRunStarted,
    PayrollRunCompleted,
    PayrollAnomalyDetected,
    PayslipGenerated,
    PaymentDisbursed,
    // Leave
    LeaveRequested,
    LeaveApproved,
    LeaveRejected,
    // Compliance
    TaxFilingPrepared,
    SsoContributionCalculated,
    PfContributionCalculated,
    // AI Agent
    AgentTaskStarted,
    AgentTaskCompleted,
    AgentEscalatedToHuman,
    // Approval workflow
    ApprovalRequested,
    ApprovalGranted,
    ApprovalDenied,
}

/// Immutable record of something that happened in the system.
/// Published to Kafka; consumed by projections, audit log, AI context builder.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainEvent {
    pub id: Uuid,
    pub event_type: EventType,
    pub tenant_id: TenantId,
    /// Correlation ID — ties together a Temporal workflow execution across services.
    pub correlation_id: Uuid,
    /// Who/what caused this event (user_id or agent_id).
    pub actor: String,
    pub payload: serde_json::Value,
    pub occurred_at: DateTime<Utc>,
}

impl DomainEvent {
    pub fn new(
        event_type: EventType,
        tenant_id: TenantId,
        correlation_id: Uuid,
        actor: impl Into<String>,
        payload: serde_json::Value,
    ) -> Self {
        Self {
            id: Uuid::now_v7(),
            event_type,
            tenant_id,
            correlation_id,
            actor: actor.into(),
            payload,
            occurred_at: Utc::now(),
        }
    }

    /// Kafka topic partition key — ensures all events for a tenant land on same partition (ordering).
    pub fn partition_key(&self) -> String {
        self.tenant_id.to_string()
    }
}
