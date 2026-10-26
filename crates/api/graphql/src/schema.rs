use async_graphql::{MergedObject, Schema, EmptySubscription, Directive, DirectiveLocation, InputValue};

use crate::resolvers::{HealthResolver, QueryRootImpl, MutationRootImpl};
use crate::types::*;

/// Root query object — merges domain resolvers.
pub type QueryRoot = MergedObject!(
    HealthResolver,
    QueryRootImpl
);

/// Root mutation object.
pub type MutationRoot = MutationRootImpl;

/// The assembled schema type alias.
pub type TalentSchema = Schema<QueryRoot, MutationRoot, EmptySubscription>;

/// Build the executable schema (used at server startup).
pub fn build_schema() -> TalentSchema {
    Schema::build(QueryRoot::default(), MutationRoot::default(), EmptySubscription)
        .directive(require_role_directive())
        .directive(require_permission_directive())
        .finish()
}

/// @requireRole directive definition for SDL
pub fn require_role_directive() -> Directive {
    Directive::new(
        "requireRole",
        vec![
            DirectiveLocation::FieldDefinition,
            DirectiveLocation::Object,
        ],
        vec![
            InputValue::new("role", async_graphql::TypeReference::named_nn(async_graphql::TypeReference::String))
                .description("Required role name (e.g., 'hr_admin', 'payroll_manager')"),
        ],
        "Requires the caller to have the specified role"
    )
}

/// @requirePermission directive definition for SDL
pub fn require_permission_directive() -> Directive {
    Directive::new(
        "requirePermission",
        vec![
            DirectiveLocation::FieldDefinition,
            DirectiveLocation::Object,
        ],
        vec![
            InputValue::new("resource", async_graphql::TypeReference::named_nn(async_graphql::TypeReference::String))
                .description("Resource name (e.g., 'employee', 'payroll_run')"),
            InputValue::new("action", async_graphql::TypeReference::named_nn(async_graphql::TypeReference::String))
                .description("Action name (e.g., 'read_all', 'create', 'approve')"),
        ],
        "Requires the caller to have the specified permission on resource"
    )
}