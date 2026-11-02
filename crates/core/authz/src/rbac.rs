use std::collections::HashMap;
use std::sync::LazyLock;

/// Static RBAC matrix per spec §3.
/// Key: (resource, action), Value: allowed role names.
static MATRIX: LazyLock<HashMap<(&'static str, &'static str), &'static [&'static str]>> =
    LazyLock::new(|| {
        HashMap::from([
            (("employee", "create"), &["hr_admin"][..]),
            (
                ("employee", "read_own_team"),
                &["hr_admin", "payroll_manager", "manager"][..],
            ),
            (
                ("employee", "read_all"),
                &["hr_admin", "payroll_manager"][..],
            ),
            (("employee", "update"), &["hr_admin"][..]),
            (("employee", "delete"), &["hr_admin"][..]),
            (
                ("employee_profile", "update_self"),
                &["hr_admin", "payroll_manager", "manager", "employee"][..],
            ),
            (
                ("payslip", "view_self"),
                &["hr_admin", "payroll_manager", "manager", "employee"][..],
            ),
            (
                ("payslip", "view_team_summary"),
                &["hr_admin", "payroll_manager", "manager"][..],
            ),
            (
                ("payslip", "view_all"),
                &["hr_admin", "payroll_manager"][..],
            ),
            (
                ("payroll_run", "calculate"),
                &["hr_admin", "payroll_manager"][..],
            ),
            (
                ("payroll_run", "approve"),
                &["hr_admin", "payroll_manager"][..],
            ),
            (
                ("payroll_run", "export_report"),
                &["hr_admin", "payroll_manager"][..],
            ),
            (
                ("payout_template", "read"),
                &["hr_admin", "payroll_manager"][..],
            ),
            (("payout_template", "create"), &["hr_admin"][..]),
            (
                ("leave_request", "submit_self"),
                &["hr_admin", "payroll_manager", "manager", "employee"][..],
            ),
            (
                ("leave_request", "view_self"),
                &["hr_admin", "payroll_manager", "manager", "employee"][..],
            ),
            (
                ("leave_request", "approve_team"),
                &["hr_admin", "manager"][..],
            ),
            (("leave_request", "view_team"), &["hr_admin", "manager"][..]),
            (
                ("ot_request", "submit_self"),
                &["hr_admin", "payroll_manager", "manager", "employee"][..],
            ),
            (
                ("ot_request", "view_self"),
                &["hr_admin", "payroll_manager", "manager", "employee"][..],
            ),
            (("ot_request", "approve_team"), &["hr_admin", "manager"][..]),
            (("ot_request", "view_team"), &["hr_admin", "manager"][..]),
            (
                ("claim", "submit_self"),
                &["hr_admin", "payroll_manager", "manager", "employee"][..],
            ),
            (("claim", "view_team"), &["hr_admin", "manager"][..]),
            (("claim", "approve_team"), &["hr_admin", "manager"][..]),
            (
                ("attendance", "manage"),
                &["hr_admin", "payroll_manager"][..],
            ),
            (
                ("attendance", "read"),
                &["hr_admin", "payroll_manager", "manager"][..],
            ),
            (
                ("attendance", "shifts"),
                &["hr_admin", "payroll_manager"][..],
            ),
            (
                ("attendance", "punch_self"),
                &["hr_admin", "payroll_manager", "manager", "employee"][..],
            ),
            (
                ("attendance", "approve"),
                &["hr_admin", "payroll_manager", "manager"][..],
            ),
            (("leave_balance", "manage"), &["hr_admin"][..]),
            (("settings", "manage_company"), &["hr_admin"][..]),
            (
                ("settings", "view_company"),
                &["hr_admin", "payroll_manager", "manager"][..],
            ),
            (("user_management", "invite"), &["hr_admin"][..]),
            (("user_management", "assign_role"), &["hr_admin"][..]),
            (("user_management", "reset_2fa"), &["hr_admin"][..]),
            (("user_management", "update"), &["hr_admin"][..]),
            (
                ("performance", "manage_direct"),
                &["hr_admin", "manager"][..],
            ),
            (("audit_log", "read"), &["hr_admin"][..]),
            (("auth", "oidc_manage"), &["hr_admin"][..]),
            (("privacy", "purge"), &["hr_admin"][..]),
            (
                ("approval_delegation", "manage"),
                &["hr_admin", "manager"][..],
            ),
            (
                ("approval_delegation", "view_self"),
                &["hr_admin", "payroll_manager", "manager", "employee"][..],
            ),
            (
                ("ai", "chat"),
                &["hr_admin", "payroll_manager", "manager"][..],
            ),
            (("ai.settings", "manage"), &["hr_admin"][..]),
        ])
    });

pub type Role = &'static str;

pub const ROLES: &[Role] = &["hr_admin", "payroll_manager", "manager", "employee"];

pub fn has_permission(roles: &[String], resource: &str, action: &str) -> bool {
    let key = (resource, action);
    MATRIX
        .get(&key)
        .is_some_and(|allowed| roles.iter().any(|r| allowed.contains(&r.as_str())))
}

/// Expand all permissions for given roles into a flat list of "resource.action" strings.
pub fn expand_permissions(roles: &[String]) -> Vec<String> {
    MATRIX
        .iter()
        .filter(|(_, allowed)| roles.iter().any(|r| allowed.contains(&r.as_str())))
        .map(|((res, act), _)| format!("{}.{}", res, act))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hr_admin_can_create_employee() {
        assert!(has_permission(&["hr_admin".into()], "employee", "create"));
    }

    #[test]
    fn employee_cannot_create_employee() {
        assert!(!has_permission(&["employee".into()], "employee", "create"));
    }

    #[test]
    fn payroll_manager_can_calculate_payroll() {
        assert!(has_permission(
            &["payroll_manager".into()],
            "payroll_run",
            "calculate"
        ));
    }

    #[test]
    fn manager_cannot_export_payroll_report() {
        assert!(!has_permission(
            &["manager".into()],
            "payroll_run",
            "export_report"
        ));
    }

    #[test]
    fn only_hr_admin_creates_payout_templates() {
        assert!(has_permission(
            &["hr_admin".to_string()],
            "payout_template",
            "create"
        ));
        for role in ["payroll_manager", "manager", "employee"] {
            assert!(
                !has_permission(&[role.to_string()], "payout_template", "create"),
                "{role} must not create payout templates"
            );
        }
        assert!(has_permission(
            &["payroll_manager".into()],
            "payout_template",
            "read"
        ));
        assert!(!has_permission(
            &["employee".into()],
            "payout_template",
            "read"
        ));
    }

    #[test]
    fn all_roles_can_view_own_payslip() {
        for role in ROLES {
            assert!(
                has_permission(&[role.to_string()], "payslip", "view_self"),
                "role {} failed",
                role
            );
        }
    }

    #[test]
    fn only_hr_admin_reads_audit_log() {
        assert!(has_permission(&["hr_admin".into()], "audit_log", "read"));
        assert!(!has_permission(
            &["payroll_manager".into()],
            "audit_log",
            "read"
        ));
        assert!(!has_permission(&["manager".into()], "audit_log", "read"));
        assert!(!has_permission(&["employee".into()], "audit_log", "read"));
    }

    #[test]
    fn unknown_resource_denied() {
        assert!(!has_permission(
            &["hr_admin".into()],
            "nonexistent",
            "do_something"
        ));
    }

    #[test]
    fn empty_roles_denied_everything() {
        assert!(!has_permission(&[], "employee", "create"));
        assert!(!has_permission(&[], "payslip", "view_self"));
    }

    #[test]
    fn expand_permissions_for_hr_admin() {
        let perms = expand_permissions(&["hr_admin".into()]);
        assert!(perms.contains(&"employee.create".to_string()));
        assert!(perms.contains(&"audit_log.read".to_string()));
        assert!(perms.len() >= 18); // hr_admin has nearly all permissions
        assert!(perms.contains(&"auth.oidc_manage".to_string()));
    }

    #[test]
    fn manager_can_manage_direct_performance_but_employee_cannot() {
        for role in ["hr_admin", "manager"] {
            assert!(
                has_permission(&[role.into()], "performance", "manage_direct"),
                "{role} should manage direct performance"
            );
        }
        for role in ["payroll_manager", "employee"] {
            assert!(
                !has_permission(&[role.into()], "performance", "manage_direct"),
                "{role} should not manage direct performance"
            );
        }
    }

    #[test]
    fn only_hr_admin_purges_privacy() {
        assert!(has_permission(&["hr_admin".into()], "privacy", "purge"));
        for role in ["payroll_manager", "manager", "employee"] {
            assert!(
                !has_permission(&[role.into()], "privacy", "purge"),
                "{role} must not purge"
            );
        }
    }

    #[test]
    fn only_hr_admin_manages_oidc_settings() {
        assert!(has_permission(&["hr_admin".into()], "auth", "oidc_manage"));
        for role in ["payroll_manager", "manager", "employee"] {
            assert!(
                !has_permission(&[role.into()], "auth", "oidc_manage"),
                "{role} must not manage OIDC settings"
            );
        }
    }

    #[test]
    fn expand_permissions_for_employee_only() {
        let perms = expand_permissions(&["employee".into()]);
        assert!(perms.contains(&"payslip.view_self".to_string()));
        assert!(perms.contains(&"leave_request.submit_self".to_string()));
        assert!(perms.contains(&"leave_request.view_self".to_string()));
        assert!(perms.contains(&"ot_request.submit_self".to_string()));
        assert!(perms.contains(&"ot_request.view_self".to_string()));
        assert!(perms.contains(&"claim.submit_self".to_string()));
        assert!(perms.contains(&"approval_delegation.view_self".to_string()));
        assert!(!perms.contains(&"employee.create".to_string()));
        assert!(!perms.contains(&"approval_delegation.manage".to_string()));
        assert_eq!(perms.len(), 9); // update_self + payslip.view_self + leave_request.submit_self + leave_request.view_self + ot_request.submit_self + ot_request.view_self + claim.submit_self + punch_self + approval_delegation.view_self
    }

    #[test]
    fn only_hr_admin_and_manager_manage_approval_delegations() {
        for role in ["hr_admin", "manager"] {
            assert!(
                has_permission(&[role.into()], "approval_delegation", "manage"),
                "{role} should manage delegations"
            );
        }
        for role in ["payroll_manager", "employee"] {
            assert!(
                !has_permission(&[role.into()], "approval_delegation", "manage"),
                "{role} should not manage delegations"
            );
        }
    }

    #[test]
    fn all_roles_can_punch_self() {
        for role in ROLES {
            assert!(
                has_permission(&[role.to_string()], "attendance", "punch_self"),
                "role {role} failed"
            );
        }
    }

    #[test]
    fn hr_admin_and_payroll_manager_can_manage_shifts() {
        for role in ["hr_admin", "payroll_manager"] {
            assert!(
                has_permission(&[role.into()], "attendance", "shifts"),
                "{role} should manage shifts"
            );
        }
    }

    #[test]
    fn employee_and_manager_cannot_manage_shifts() {
        for role in ["employee", "manager"] {
            assert!(
                !has_permission(&[role.into()], "attendance", "shifts"),
                "{role} should not manage shifts"
            );
        }
    }

    #[test]
    fn hr_admin_payroll_manager_and_manager_can_approve_attendance_day() {
        for role in ["hr_admin", "payroll_manager", "manager"] {
            assert!(
                has_permission(&[role.into()], "attendance", "approve"),
                "{role} should approve attendance days"
            );
        }
    }

    #[test]
    fn employee_cannot_approve_attendance_day() {
        assert!(!has_permission(
            &["employee".into()],
            "attendance",
            "approve"
        ));
    }

    #[test]
    fn ai_permissions_follow_role_lists() {
        assert!(!has_permission(&["employee".into()], "ai", "chat"));
        for role in ["hr_admin", "payroll_manager", "manager"] {
            assert!(
                has_permission(&[role.into()], "ai", "chat"),
                "{role} should chat"
            );
        }
        assert!(has_permission(
            &["hr_admin".into()],
            "ai.settings",
            "manage"
        ));
        for role in ["payroll_manager", "manager", "employee"] {
            assert!(
                !has_permission(&[role.into()], "ai.settings", "manage"),
                "{role} must not manage AI settings"
            );
        }
    }
}
