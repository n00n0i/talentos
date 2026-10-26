//! TalentOS Memory System - Test and Example

use talentos_memory::{
    MemoryManager, MemoryManagerConfig, MemoryQuery, LayerMetadata,
    HindsightMemory, HindsightConfig,
    SemanticaMemory, SemanticaConfig,
    ThaiHROntology, PersonalAllowances,
    MemoryManagerTrait, ValidationResult,
};
use talentos_kernel::{TenantId, EntityId};
use uuid::Uuid;
use chrono::Utc;
use serde_json::json;
use rust_decimal_macros::dec;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🧠 TalentOS 7-Layered Memory System Demo");
    println!("=========================================\n");

    // Create a test tenant
    let tenant_id = uuid::Uuid::new_v4();
    
    // Configure memory system
    let config = MemoryManagerConfig {
        tenant_id: talentos_kernel::TenantId(tenant_id),
        ..Default::default()
    };

    // Initialize memory manager
    println!("🔧 Initializing 7-Layer Memory System...");
    let mut memory = MemoryManager::new(config);
    memory.initialize().await?;
    println!("✅ Memory system initialized\n");

    // Demo 1: Store experience (L2 - Episodic)
    println!("📝 Demo 1: Recording Agent Experience (L2)");
    memory.record_experience(
        "Processed payroll for 127 employees in October 2026. Detected 2 anomalies: OT excess for employee EMP-001 and SSO cap hit for employee EMP-045.",
        Some("Payroll processing completed with anomaly detection".to_string()),
        LayerMetadata {
            tenant_id: Some(talentos_kernel::TenantId(Uuid::new_v4())),
            tags: vec!["experience".to_string(), "payroll".to_string(), "anomaly".to_string()],
            importance: 0.9,
            source: "payroll_agent".to_string(),
            ..Default::default()
        },
    ).await?;
    println!("✅ Experience stored in Episodic Memory (L2)\n");

    // Demo 2: Store world fact (L3)
    println!("📝 Demo 2: Storing World Fact (L3)");
    memory.store(
        "thai_tax_brackets_2026",
        &json!({
            "brackets": [
                {"lower": 0, "upper": 150000, "rate": 0.0},
                {"lower": 150001, "upper": 300000, "rate": 0.05},
                {"lower": 300001, "upper": 500000, "rate": 0.10},
                {"lower": 500001, "upper": 750000, "rate": 0.15},
                {"lower": 750001, "upper": 1000000, "rate": 0.20},
                {"lower": 1000001, "upper": 2000000, "rate": 0.25},
                {"lower": 2000001, "upper": 5000000, "rate": 0.30},
                {"lower": 5000001, "upper": null, "rate": 0.35},
            ],
            "year": 2026,
            "currency": "THB"
        }),
        LayerMetadata {
            tags: vec!["fact".to_string(), "tax".to_string(), "thai_law".to_string()],
            importance: 1.0,
            source: "thai_revenue_dept".to_string(),
            ..Default::default()
        },
    ).await?;
    println!("✅ Thai tax brackets 2026 stored in Semantic Memory (L3)\n");

    // Demo 3: Add entity to Context Graph (L4)
    println!("📝 Demo 3: Adding Entity to Context Graph (L4)");
    let semantica = crate::semantica_integration::SemanticaMemory::new(
        crate::semantica_integration::SemanticaConfig::default(),
        talentos_kernel::TenantId(uuid::Uuid::new_v4()),
    );
    semantica.add_entity(
        "emp-001",
        "Employee",
        json!({
            "employee_code": "EMP-001",
            "first_name_th": "สมชาย",
            "last_name_th": "ใจดี",
            "base_salary": 85000,
            "department": "Engineering"
        }),
    ).await?;
    println!("✅ Employee entity added to Context Graph (L4)\n");

    // Demo 4: Record decision (L5)
    println!("📝 Demo 4: Recording Decision with Causal Chain (L5)");
    let decision_id = memory.record_decision(
        "payroll_anomaly_resolution",
        "OT anomaly detected for employee EMP-001 (58 hours vs team avg 12h)",
        "Employee worked excessive OT due to project deadline. Recommend workload redistribution.",
        "flagged_for_review",
        0.92,
        "payroll_manager",
        vec![],
    ).await?;
    println!("✅ Decision recorded with ID: {}\n", decision_id);

    // Demo 6: Thai HR Tax Calculation
    println!("📝 Demo 6: Thai Tax Calculation (Thai HR Ontology)");
    let ontology = ThaiHROntology::new();
    let allowances = PersonalAllowances {
        personal: dec!(60000),
        spouse: true,
        children: 2,
        insurance: dec!(15000),
        pension: dec!(50000),
    };
    let annual_income = dec!(1_000_000);
    let tax = ontology.calculate_pnd_tax(annual_income, allowances)?;
    println!("✅ Annual income: {:.2} THB, Tax: {:.2} THB\n", annual_income, tax);

    // Demo 7: SSO Calculation
    println!("📝 Demo 7: SSO Calculation");
    let monthly_wage = dec!(85000);
    let (emp_sso, employer_sso) = ontology.calculate_sso(monthly_wage);
    println!("✅ Monthly wage: {:.2} THB, Employee SSO: {:.2}, Employer SSO: {:.2}\n", monthly_wage, emp_sso, employer_sso);

    // Demo 7: Cross-layer Query
    println!("📝 Demo 8: Cross-Layer Query");
    let results = memory.retrieve(&MemoryQuery {
        query_text: Some("payroll anomaly".to_string()),
        limit: 5,
        ..Default::default()
    }).await?;
    println!("✅ Found {} results across layers\n", results.len());
    for item in &results {
        println!("  - {} (layer: {}, importance: {:.2})", item.key, item.metadata.tags.join(","), item.metadata.importance);
    }

    // Stats
    println!("\n📊 Memory System Stats:");
    let stats = memory.stats().await?;
    println!("  Total items: {}", stats.total_items);
    println!("  Total size: {} bytes", stats.total_size_bytes);
    for layer_stat in &stats.layer_stats {
        println!("  Layer {} ({}): {} items, hit_rate: {:.2}", layer_stat.base.layer_id, layer_stat.base.layer_name, layer_stat.base.item_count, layer_stat.base.hit_rate);
    }

    println!("\n✅ All demos completed successfully!");
    Ok(())
}