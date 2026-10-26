//! In-process agent loop: OBSERVE → PLAN → ACT → REFLECT, with replanning and a
//! human-approval gate for irreversible actions.
//!
//! Durable execution (Temporal) is intentionally not wired yet: this loop is a
//! plain async function so it is testable today. When Temporal lands, the four
//! phases become activities and this loop becomes the workflow body — the
//! `AgentPhase` boundary below is exactly where that seam goes.

use std::sync::Arc;
use talentos_db::TenantPool;
use talentos_kernel::shared::{
    ActResult, ActionPlan, AgentContext, AgentError, AgentResult, Observation, Reflection,
    WorkflowPhase,
};
use talentos_model_gw::HuakatiClient;
use talentos_rag::HybridSearch;
use talentos_tools::ToolRegistry;

use crate::activities::{ActActivity, ObserveActivity, PlanActivity, ReflectActivity};

/// Everything the loop needs, assembled once per run.
pub struct AgentRunner {
    pub context: AgentContext,
    pub model_gw: HuakatiClient,
    pub db: Arc<TenantPool>,
    pub hybrid_search: HybridSearch,
    pub tool_registry: Arc<ToolRegistry>,
    pub max_cycles: usize,
}

/// One completed cycle, kept for the audit trail.
#[derive(Debug, Clone)]
pub struct CycleTrace {
    pub phase: WorkflowPhase,
    pub observation: Observation,
    pub plan: ActionPlan,
    pub result: ActResult,
    pub reflection: Reflection,
}

/// Outcome of a full run.
#[derive(Debug)]
pub struct RunReport {
    pub result: AgentResult,
    pub cycles: Vec<CycleTrace>,
    pub approvals_requested: Vec<String>,
}

impl AgentRunner {
    /// Called before executing an irreversible tool. Returning `false` cancels
    /// the plan and ends the run with [`AgentResult::ApprovalRejected`].
    ///
    /// Default wiring asks the caller (API/CLI) to decide; a Temporal signal
    /// replaces this closure once durable execution lands.
    pub async fn run<F, Fut>(&self, task: &str, mut approve: F) -> RunReport
    where
        F: FnMut(&str) -> Fut,
        Fut: std::future::Future<Output = bool>,
    {
        let mut cycles = Vec::new();
        let mut approvals_requested = Vec::new();

        for _cycle in 0..self.max_cycles {
            let observation = match self.observe(task).await {
                Ok(o) => o,
                Err(e) => return self.failed(e.to_string(), cycles, approvals_requested),
            };

            let plan = match self.plan(task, &observation).await {
                Ok(p) => p,
                Err(e) => return self.failed(e.to_string(), cycles, approvals_requested),
            };

            // Human gate: ask for every irreversible step before running any of them.
            if plan.requires_approval {
                let blocked = match plan.steps.iter().find(|s| s.is_destructive) {
                    Some(step) => step.tool_name.clone(),
                    None => plan.approval_tool.clone().unwrap_or_default(),
                };
                approvals_requested.push(blocked.clone());
                if !approve(&blocked).await {
                    return RunReport {
                        result: AgentResult::ApprovalRejected(format!("human rejected {blocked}")),
                        cycles,
                        approvals_requested,
                    };
                }
            }

            let result = match self.act(&plan).await {
                Ok(r) => r,
                Err(e) => return self.failed(e.to_string(), cycles, approvals_requested),
            };

            let reflection = match self.reflect(&plan, &result).await {
                Ok(r) => r,
                Err(e) => return self.failed(e.to_string(), cycles, approvals_requested),
            };

            let done = reflection.score >= 0.8 && result.is_complete;
            cycles.push(CycleTrace {
                phase: WorkflowPhase::Reflect,
                observation,
                plan,
                result,
                reflection,
            });

            if done {
                return RunReport {
                    result: AgentResult::Success(
                        cycles.last().expect("cycle just pushed").result.clone(),
                    ),
                    cycles,
                    approvals_requested,
                };
            }
        }

        RunReport {
            result: AgentResult::MaxCyclesExceeded,
            cycles,
            approvals_requested,
        }
    }

    async fn observe(&self, task: &str) -> Result<Observation, AgentError> {
        ObserveActivity {
            context: self.context.clone(),
            model_gw: self.model_gw.clone(),
            db: self.db.clone(),
            hybrid_search: HybridSearch::new(self.model_gw.clone(), self.db.pool().clone()),
            tool_registry: self.tool_registry.clone(),
        }
        .execute(task)
        .await
    }

    async fn plan(&self, task: &str, observation: &Observation) -> Result<ActionPlan, AgentError> {
        PlanActivity {
            context: self.context.clone(),
            model_gw: self.model_gw.clone(),
            tool_registry: self.tool_registry.clone(),
        }
        .execute(task, observation)
        .await
    }

    async fn act(&self, plan: &ActionPlan) -> Result<ActResult, AgentError> {
        ActActivity {
            context: self.context.clone(),
            model_gw: self.model_gw.clone(),
            db: self.db.clone(),
            tool_registry: self.tool_registry.clone(),
        }
        .execute(plan)
        .await
    }

    async fn reflect(
        &self,
        plan: &ActionPlan,
        result: &ActResult,
    ) -> Result<Reflection, AgentError> {
        ReflectActivity {
            context: self.context.clone(),
            model_gw: self.model_gw.clone(),
        }
        .execute(plan, result)
        .await
    }

    fn failed(
        &self,
        message: String,
        cycles: Vec<CycleTrace>,
        approvals_requested: Vec<String>,
    ) -> RunReport {
        RunReport {
            result: AgentResult::Failed(message),
            cycles,
            approvals_requested,
        }
    }
}

/// Input for starting a run (kept for parity with the future Temporal workflow).
pub type AgentInput = talentos_kernel::shared::AgentInput;
