use std::time::Duration;

use temporal_sdk::{WorkflowContext, WorkflowResult, act, query, signal, workflow};
use uuid::Uuid;

use crate::activities::{ObserveActivity, PlanActivity, ActActivity, ReflectActivity};
use crate::states::{WorkflowState, WorkflowPhase, Observation, ActionPlan, ActResult, Reflection, AgentResult, AgentError, HumanApprovalSignal, HumanRejectionSignal, AdditionalContextSignal};
use talentos_kernel::shared::AgentContext;

/// Agent workflow definition
#[workflow]
pub struct AgentWorkflow {
    pub agent_type: String,
    pub task: String,
    pub context: AgentContext,
    pub state: WorkflowState,
}

impl AgentWorkflow {
    /// Main workflow execution
    #[workflow::run]
    pub async fn execute(ctx: &WorkflowContext, input: AgentInput) -> WorkflowResult<AgentResult> {
        let mut workflow = Self {
            agent_type: input.agent_type,
            task: input.task,
            context: input.context,
            state: WorkflowState::new(),
        };

        let max_cycles = 3;
        let cycle_timeout = Duration::from_secs(600); // 10 min per cycle

        loop {
            // Check if we should stop
            if workflow.state.cycle_count >= max_cycles {
                let last_reflection = workflow.state.reflections.last();
                if let Some(r) = last_reflection {
                    if r.score < 0.3 {
                        return Ok(AgentResult::Failed("Max cycles exceeded with low score".to_string()));
                    }
                }
            }

            // Execute one cycle with timeout
            let cycle_result = tokio::time::timeout(cycle_timeout, async {
                // OBSERVE
                let observation = workflow.observe(&ctx).await?;
                workflow.state.record_step("OBSERVE", observation.clone());
                workflow.state.phase = WorkflowPhase::Observe;

                // PLAN
                let plan = workflow.plan(&ctx, &observation).await?;
                workflow.state.record_step("PLAN", plan.clone());
                workflow.state.phase = WorkflowPhase::Plan;

                // ACT (may pause for human approval)
                let result = workflow.act(&ctx, &plan).await?;
                workflow.state.record_step("ACT", result.clone());
                workflow.state.phase = WorkflowPhase::Act;

                // REFLECT
                let reflection = workflow.reflect(&plan, &result).await?;
                workflow.state.record_step("REFLECT", reflection.clone());
                workflow.state.phase = WorkflowPhase::Reflect;

                workflow.state.cycle_count += 1;

                // Check if done
                if reflection.score >= 0.8 && result.is_complete {
                    return Ok::<AgentResult, AgentError>(AgentResult::Success(result));
                }

                if reflection.score < 0.3 && workflow.state.cycle_count >= max_cycles {
                    return Ok::<AgentResult, AgentError>(AgentResult::Failed("Max cycles exceeded".to_string()));
                }

                // Continue to next cycle (REPLAN)
                workflow.state.phase = WorkflowPhase::Replan;
                Ok::<AgentResult, AgentError>(AgentResult::Failed("Continue".to_string()))
            }).await;

            match cycle_result {
                Ok(Ok(AgentResult::Success(result))) => return Ok(AgentResult::Success(result)),
                Ok(Ok(AgentResult::Failed(msg))) if msg != "Continue" => return Ok(AgentResult::Failed(msg)),
                Ok(Err(e)) => return Err(e),
                Err(_) => {
                    // Timeout - record and continue if not max cycles
                    if workflow.state.cycle_count >= max_cycles {
                        return Ok(AgentResult::Failed("Cycle timeout, max cycles reached".to_string()));
                    }
                    workflow.state.cycle_count += 1;
                }
            }
        }
    }

    async fn observe(&self, ctx: &WorkflowContext) -> Result<Observation, AgentError> {
        let activity = ObserveActivity::new(self.context.clone());
        activity.execute(ctx).await
    }

    async fn plan(&self, ctx: &WorkflowContext, observation: &Observation) -> Result<ActionPlan, AgentError> {
        let activity = PlanActivity::new(self.context.clone());
        activity.execute(ctx, observation, &self.task).await
    }

    async fn act(&self, ctx: &WorkflowContext, plan: &ActionPlan) -> Result<ActResult, AgentError> {
        let activity = ActActivity::new(self.context.clone());
        activity.execute(ctx, plan).await
    }

    async fn reflect(&self, plan: &ActionPlan, result: &ActResult) -> Result<Reflection, AgentError> {
        let activity = ReflectActivity::new(self.context.clone());
        activity.execute(plan, result).await
    }

    /// Get current workflow state
    #[query(name = "get_state")]
    pub fn get_state(&self) -> WorkflowState {
        self.state.clone()
    }

    /// Get current plan
    #[query(name = "get_plan")]
    pub fn get_plan(&self) -> Option<ActionPlan> {
        self.state.plans.last().cloned()
    }

    /// Get execution history
    #[query(name = "get_history")]
    pub fn get_history(&self) -> Vec<crate::states::StepSnapshot> {
        self.state.history.clone()
    }

    /// Handle human approval signal
    #[signal(name = "human_approve")]
    pub fn human_approve(&mut self, signal: HumanApprovalSignal) {
        self.state.handle_approval(signal);
    }

    /// Handle human rejection signal
    #[signal(name = "human_reject")]
    pub fn human_reject(&mut self, signal: HumanRejectionSignal) {
        self.state.handle_rejection(signal);
    }

    /// Handle additional context signal
    #[signal(name = "provide_context")]
    pub fn provide_context(&mut self, signal: AdditionalContextSignal) {
        self.state.add_context(signal.context);
    }
}

/// Input for starting an agent workflow
pub type AgentInput = talentos_kernel::shared::AgentInput;