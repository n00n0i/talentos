//! Thai HR Ontology - Domain knowledge for Thai HR/Payroll compliance

use crate::talentos_kernel::{TenantId, EntityId};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use chrono::NaiveDate;
use serde_json::Value;
use std::collections::HashMap;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

/// Thai HR Concept definition
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ThaiHRConcept {
    pub id: String,
    pub label_th: String,
    pub label_en: String,
    pub definition_th: String,
    pub definition_en: String,
    pub concept_type: ConceptType,
    pub properties: HashMap<String, PropertyDefinition>,
    pub relationships: Vec<RelationshipDefinition>,
    pub shacl_shapes: Vec<String>,
    pub applicable_laws: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ConceptType {
    Person,
    Organization,
    Event,
    Document,
    Regulation,
    Rule,
    Vocabulary,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PropertyDefinition {
    pub name: String,
    pub label_th: String,
    pub label_en: String,
    pub data_type: DataType,
    pub required: bool,
    pub constraints: Vec<String>,
    pub validation_rules: Vec<ValidationRule>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum DataType {
    String,
    Integer,
    Decimal,
    Date,
    DateTime,
    Boolean,
    UUID,
    Enum(Vec<String>),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ValidationRule {
    pub rule_type: ValidationRuleType,
    pub parameters: HashMap<String, Value>,
    pub error_message_th: String,
    pub error_message_en: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum ValidationRuleType {
    MinValue,
    MaxValue,
    MinLength,
    MaxLength,
    Pattern,
    Range,
    Custom,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RelationshipDefinition {
    pub predicate: String,
    pub target_concept: String,
    pub cardinality: Cardinality,
    pub inverse: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum Cardinality {
    OneToOne,
    OneToMany,
    ManyToOne,
    ManyToMany,
}

/// Thai HR Ontology - Core domain knowledge for Thai HR/Payroll
pub struct ThaiHROntology {
    concepts: HashMap<String, ThaiHRConcept>,
    tax_brackets_2026: Vec<TaxBracket>,
    sso_rules: SSORules,
    pf_rules: PFRules,
    severance_rules: Vec<SeveranceRule>,
    ot_rules: OTRules,
    leave_types: Vec<LeaveType>,
}

impl ThaiHROntology {
    pub fn new() -> Self {
        let mut ontology = Self {
            concepts: HashMap::new(),
            tax_brackets_2026: Vec::new(),
            sso_rules: SSORules::default(),
            pf_rules: PFRules::default(),
            severance_rules: Vec::new(),
            ot_rules: OTRules::default(),
            leave_types: Vec::new(),
        };

        ontology.initialize_core_concepts();
        ontology.initialize_thai_tax_2026();
        ontology.initialize_sso_rules();
        ontology.initialize_pf_rules();
        ontology.initialize_severance_rules();
        ontology.initialize_ot_rules();
        ontology.initialize_leave_types();

        ontology
    }

    fn initialize_core_concepts(&mut self) {
        // Employee concept
        let mut employee = ThaiHRConcept {
            id: "thaihr:Employee".to_string(),
            label_th: "พนักงาน".to_string(),
            label_en: "Employee".to_string(),
            definition_th: "บุคคลที่ทำงานภายใต้สัญญาจ้างงานตามกฎหมายคุ้มครองแรงงาน".to_string(),
            definition_en: "Person employed under an employment contract per Labor Protection Act".to_string(),
            concept_type: ConceptType::Person,
            properties: HashMap::new(),
            relationships: Vec::new(),
            shacl_shapes: vec!["thaihr:EmployeeShape".to_string()],
            applicable_laws: vec![
                "Labor Protection Act B.E. 2541".to_string(),
                "Labor Relations Act B.E. 2518".to_string(),
            ],
        };

        employee.properties.insert("employee_code".to_string(), PropertyDefinition {
            name: "employee_code".to_string(),
            label_th: "รหัสพนักงาน".to_string(),
            label_en: "Employee Code".to_string(),
            data_type: DataType::String,
            required: true,
            constraints: vec!["unique".to_string(), "pattern:^[A-Z0-9]{6,10}$".to_string()],
            validation_rules: vec![ValidationRule {
                rule_type: ValidationRuleType::Pattern,
                parameters: [("pattern".to_string(), Value::String(r"^[A-Z0-9]{6,10}$".to_string()))].into_iter().collect(),
                error_message_th: "รหัสพนักงานต้องเป็นตัวอักษรตัวใหญ่และตัวเลข 6-10 ตัว".to_string(),
                error_message_en: "Employee code must be 6-10 uppercase alphanumeric characters".to_string(),
            }],
        });

        employee.properties.insert("first_name_th".to_string(), PropertyDefinition {
            name: "first_name_th".to_string(),
            label_th: "ชื่อ (ไทย)".to_string(),
            label_en: "First Name (TH)".to_string(),
            data_type: DataType::String,
            required: true,
            constraints: vec!["max_length:100".to_string()],
            validation_rules: vec![],
        });

        employee.properties.insert("last_name_th".to_string(), PropertyDefinition {
            name: "last_name_th".to_string(),
            label_th: "นามสกุล (ไทย)".to_string(),
            label_en: "Last Name (TH)".to_string(),
            data_type: DataType::String,
            required: true,
            constraints: vec!["max_length:100".to_string()],
            validation_rules: vec![],
        });

        employee.properties.insert("first_name_en".to_string(), PropertyDefinition {
            name: "first_name_en".to_string(),
            label_th: "ชื่อ (อังกฤษ)".to_string(),
            label_en: "First Name (EN)".to_string(),
            data_type: DataType::String,
            required: false,
            constraints: vec!["max_length:100".to_string()],
            validation_rules: vec![],
        });

        employee.properties.insert("last_name_en".to_string(), PropertyDefinition {
            name: "last_name_en".to_string(),
            label_th: "นามสกุล (อังกฤษ)".to_string(),
            label_en: "Last Name (EN)".to_string(),
            data_type: DataType::String,
            required: false,
            constraints: vec!["max_length:100".to_string()],
            validation_rules: vec![],
        });

        employee.properties.insert("national_id".to_string(), PropertyDefinition {
            name: "national_id".to_string(),
            label_th: "เลขบัตรประชาชน".to_string(),
            label_en: "National ID".to_string(),
            data_type: DataType::String,
            required: true,
            constraints: vec!["pattern:^[0-9]{13}$".to_string(), "unique".to_string()],
            validation_rules: vec![ValidationRule {
                rule_type: ValidationRuleType::Pattern,
                parameters: [("pattern".to_string(), Value::String(r"^[0-9]{13}$".to_string()))].into_iter().collect(),
                error_message_th: "เลขบัตรประชาชนต้องเป็นตัวเลข 13 หลัก".to_string(),
                error_message_en: "National ID must be 13 digits".to_string(),
            }],
        });

        employee.properties.insert("birth_date".to_string(), PropertyDefinition {
            name: "birth_date".to_string(),
            label_th: "วันเกิด".to_string(),
            label_en: "Birth Date".to_string(),
            data_type: DataType::Date,
            required: true,
            constraints: vec!["before:today".to_string(), "age>=15".to_string()],
            validation_rules: vec![ValidationRule {
                rule_type: ValidationRuleType::Custom,
                parameters: [("min_age".to_string(), Value::Number(15.into()))].into_iter().collect(),
                error_message_th: "อายุต้องไม่น้อยกว่า 15 ปี".to_string(),
                error_message_en: "Age must be at least 15 years".to_string(),
            }],
        });

        employee.properties.insert("hire_date".to_string(), PropertyDefinition {
            name: "hire_date".to_string(),
            label_th: "วันเริ่มงาน".to_string(),
            label_en: "Hire Date".to_string(),
            data_type: DataType::Date,
            required: true,
            constraints: vec!["before_or_equal:today".to_string()],
            validation_rules: vec![],
        });

        employee.properties.insert("position_title".to_string(), PropertyDefinition {
            name: "position_title".to_string(),
            label_th: "ตำแหน่ง".to_string(),
            label_en: "Position Title".to_string(),
            data_type: DataType::String,
            required: true,
            constraints: vec!["max_length:100".to_string()],
            validation_rules: vec![],
        });

        employee.properties.insert("department".to_string(), PropertyDefinition {
            name: "department".to_string(),
            label_th: "แผนก".to_string(),
            label_en: "Department".to_string(),
            data_type: DataType::String,
            required: true,
            constraints: vec!["max_length:100".to_string()],
            validation_rules: vec![],
        });

        employee.properties.insert("base_salary".to_string(), PropertyDefinition {
            name: "base_salary".to_string(),
            label_th: "เงินเดือนฐาน".to_string(),
            label_en: "Base Salary".to_string(),
            data_type: DataType::Decimal,
            required: true,
            constraints: vec!["min:0".to_string(), "max:10000000".to_string()],
            validation_rules: vec![ValidationRule {
                rule_type: ValidationRuleType::Range,
                parameters: [("min".to_string(), Value::String("0".to_string())), ("max".to_string(), Value::String("10000000".to_string()))].into_iter().collect(),
                error_message_th: "เงินเดือนฐานต้องอยู่ระหว่าง 0 - 10,000,000 บาท".to_string(),
                error_message_en: "Base salary must be between 0 - 10,000,000 THB".to_string(),
            }],
        });

        employee.properties.insert("bank_account_number".to_string(), PropertyDefinition {
            name: "bank_account_number".to_string(),
            label_th: "เลขบัญชีธนาคาร".to_string(),
            label_en: "Bank Account Number".to_string(),
            data_type: DataType::String,
            required: true,
            constraints: vec!["pattern:^[0-9]{10,17}$".to_string()],
            validation_rules: vec![ValidationRule {
                rule_type: ValidationRuleType::Pattern,
                parameters: [("pattern".to_string(), Value::String(r"^[0-9]{10,17}$".to_string()))].into_iter().collect(),
                error_message_th: "เลขบัญชีต้องเป็นตัวเลข 10-17 หลัก".to_string(),
                error_message_en: "Bank account must be 10-17 digits".to_string(),
            }],
        });

        employee.properties.insert("bank_name".to_string(), PropertyDefinition {
            name: "bank_name".to_string(),
            label_th: "ชื่อธนาคาร".to_string(),
            label_en: "Bank Name".to_string(),
            data_type: DataType::String,
            required: true,
            constraints: vec![],
            validation_rules: vec![],
        });

        employee.properties.insert("tax_filing_status".to_string(), PropertyDefinition {
            name: "tax_filing_status".to_string(),
            label_th: "สถานะการยื่นภาษี".to_string(),
            label_en: "Tax Filing Status".to_string(),
            data_type: DataType::Enum(vec!["1".to_string(), "2".to_string(), "3".to_string(), "4".to_string(), "5".to_string()]),
            required: true,
            constraints: vec![],
            validation_rules: vec![],
        });

        employee.properties.insert("sso_number".to_string(), PropertyDefinition {
            name: "sso_number".to_string(),
            label_th: "เลขประกันสังคม".to_string(),
            label_en: "SSO Number".to_string(),
            data_type: DataType::String,
            required: false,
            constraints: vec!["pattern:^[0-9]{1,13}$".to_string()],
            validation_rules: vec![],
        });

        employee.properties.insert("pf_member".to_string(), PropertyDefinition {
            name: "pf_member".to_string(),
            label_th: "สมาชิกกองทุนสำรองเลี้ยงชีพ".to_string(),
            label_en: "PF Member".to_string(),
            data_type: DataType::Boolean,
            required: true,
            constraints: vec![],
            validation_rules: vec![],
        });

        employee.properties.insert("status".to_string(), PropertyDefinition {
            name: "status".to_string(),
            label_th: "สถานะ".to_string(),
            label_en: "Status".to_string(),
            data_type: DataType::Enum(vec!["active".to_string(), "probation".to_string(), "leave".to_string(), "terminated".to_string()]),
            required: true,
            constraints: vec![],
            validation_rules: vec![],
        });

        employee.relationships = vec![
            RelationshipDefinition {
                predicate: "thaihr:belongsTo".to_string(),
                target_concept: "thaihr:Department".to_string(),
                cardinality: Cardinality::ManyToOne,
                inverse: Some("thaihr:hasEmployee".to_string()),
            },
            RelationshipDefinition {
                predicate: "thaihr:holdsPosition".to_string(),
                target_concept: "thaihr:Position".to_string(),
                cardinality: Cardinality::ManyToOne,
                inverse: Some("thaihr:heldBy".to_string()),
            },
            RelationshipDefinition {
                predicate: "thaihr:participatesIn".to_string(),
                target_concept: "thaihr:PayrollRun".to_string(),
                cardinality: Cardinality::ManyToMany,
                inverse: Some("thaihr:hasParticipant".to_string()),
            },
        ];

        self.concepts.insert("thaihr:Employee".to_string(), employee);

        // Department concept
        let mut department = ThaiHRConcept {
            id: "thaihr:Department".to_string(),
            label_th: "แผนก".to_string(),
            label_en: "Department".to_string(),
            definition_th: "หน่วยงานภายในองค์กร".to_string(),
            definition_en: "Organizational unit within the company".to_string(),
            concept_type: ConceptType::Organization,
            properties: HashMap::new(),
            relationships: vec![
                RelationshipDefinition {
                    predicate: "thaihr:hasEmployee".to_string(),
                    target_concept: "thaihr:Employee".to_string(),
                    cardinality: Cardinality::OneToMany,
                    inverse: Some("thaihr:belongsTo".to_string()),
                },
            ],
            shacl_shapes: vec!["thaihr:DepartmentShape".to_string()],
            applicable_laws: vec![],
        };

        department.properties.insert("name".to_string(), PropertyDefinition {
            name: "name".to_string(),
            label_th: "ชื่อแผนก".to_string(),
            label_en: "Department Name".to_string(),
            data_type: DataType::String,
            required: true,
            constraints: vec!["unique".to_string()],
            validation_rules: vec![],
        });

        department.properties.insert("cost_center".to_string(), PropertyDefinition {
            name: "cost_center".to_string(),
            label_th: "ศูนย์ต้นทุน".to_string(),
            label_en: "Cost Center".to_string(),
            data_type: DataType::String,
            required: true,
            constraints: vec!["unique".to_string()],
            validation_rules: vec![],
        });

        self.concepts.insert("thaihr:Department".to_string(), department);

        // Position concept
        let mut position = ThaiHRConcept {
            id: "thaihr:Position".to_string(),
            label_th: "ตำแหน่ง".to_string(),
            label_en: "Position".to_string(),
            definition_th: "ตำแหน่งงานภายในองค์กร".to_string(),
            definition_en: "Job position within the organization".to_string(),
            concept_type: ConceptType::Organization,
            properties: HashMap::new(),
            relationships: vec![
                RelationshipDefinition {
                    predicate: "thaihr:heldBy".to_string(),
                    target_concept: "thaihr:Employee".to_string(),
                    cardinality: Cardinality::OneToMany,
                    inverse: Some("thaihr:holdsPosition".to_string()),
                },
            ],
            shacl_shapes: vec!["thaihr:PositionShape".to_string()],
            applicable_laws: vec![],
        };

        position.properties.insert("title".to_string(), PropertyDefinition {
            name: "title".to_string(),
            label_th: "ชื่อตำแหน่ง".to_string(),
            label_en: "Position Title".to_string(),
            data_type: DataType::String,
            required: true,
            constraints: vec!["unique".to_string()],
            validation_rules: vec![],
        });

        position.properties.insert("level".to_string(), PropertyDefinition {
            name: "level".to_string(),
            label_th: "ระดับ".to_string(),
            label_en: "Level".to_string(),
            data_type: DataType::Integer,
            required: true,
            constraints: vec!["min:1".to_string(), "max:10".to_string()],
            validation_rules: vec![],
        });

        self.concepts.insert("thaihr:Position".to_string(), position);
    }

    fn initialize_thai_tax_2026(&mut self) {
        self.tax_brackets_2026 = vec![
            TaxBracket { lower: dec!(0), upper: Some(dec!(150000)), rate: dec!(0.00) },
            TaxBracket { lower: dec!(150001), upper: Some(dec!(300000)), rate: dec!(0.05) },
            TaxBracket { lower: dec!(300001), upper: Some(dec!(500000)), rate: dec!(0.10) },
            TaxBracket { lower: dec!(500001), upper: Some(dec!(750000)), rate: dec!(0.15) },
            TaxBracket { lower: dec!(750001), upper: Some(dec!(1000000)), rate: dec!(0.20) },
            TaxBracket { lower: dec!(1000001), upper: Some(dec!(2000000)), rate: dec!(0.25) },
            TaxBracket { lower: dec!(2000001), upper: Some(dec!(5000000)), rate: dec!(0.30) },
            TaxBracket { lower: dec!(5000001), upper: None, rate: dec!(0.35) },
        ];
    }

    fn initialize_sso_rules(&mut self) {
        self.sso_rules = SSORules {
            employee_rate: dec!(0.05),
            employer_rate: dec!(0.05),
            min_wage_base: dec!(1650),
            max_wage_base: dec!(15000),
        };
    }

    fn initialize_pf_rules(&mut self) {
        self.pf_rules = PFRules {
            min_employee_rate: dec!(0.02),
            max_employee_rate: dec!(0.15),
            employer_match_rate: dec!(0.05),
        };
    }

    fn initialize_severance_rules(&mut self) {
        self.severance_rules = vec![
            SeveranceRule { min_days: 120, max_days: Some(1095), severance_days: 30 },   // 120 days - < 1 year
            SeveranceRule { min_days: 365, max_days: Some(1095), severance_days: 90 },   // 1 - 3 years
            SeveranceRule { min_days: 1095, max_days: Some(2190), severance_days: 180 }, // 3 - 6 years
            SeveranceRule { min_days: 2190, max_days: Some(3650), severance_days: 240 }, // 6 - 10 years
            SeveranceRule { min_days: 3650, max_days: Some(7300), severance_days: 300 }, // 10 - 20 years
            SeveranceRule { min_days: 7300, max_days: None, severance_days: 400 },       // 20+ years
        ];
    }

    fn initialize_ot_rules(&mut self) {
        self.ot_rules = OTRules {
            weekday_rate: dec!(1.5),
            weekend_rate: dec!(2.0),
            holiday_rate: dec!(3.0),
            max_hours_per_week: dec!(36),
        };
    }

    fn initialize_leave_types(&mut self) {
        self.leave_types = vec![
            LeaveType { code: "annual".to_string(), label_th: "ลาปริญญา", label_en: "Annual Leave", max_days: 6, carry_forward: true },
            LeaveType { code: "sick".to_string(), label_th: "ลาป่วย", label_en: "Sick Leave", max_days: 30, carry_forward: false },
            LeaveType { code: "personal".to_string(), label_th: "ลากิจ", label_en: "Personal Leave", max_days: 3, carry_forward: false },
            LeaveType { code: "maternity".to_string(), label_th: "ลาคลอด", label_en: "Maternity Leave", max_days: 98, carry_forward: false },
            LeaveType { code: "paternity".to_string(), label_th: "ลาพ่อคลอด", label_en: "Paternity Leave", max_days: 15, carry_forward: false },
            LeaveType { code: "bereavement".to_string(), label_th: "ลาศพ", label_en: "Bereavement Leave", max_days: 3, carry_forward: false },
            LeaveType { code: "unpaid".to_string(), label_th: "ลาค้างค่าแรง", label_en: "Unpaid Leave", max_days: 0, carry_forward: false },
        ];
    }

    pub fn get_concept(&self, id: &str) -> Option<&ThaiHRConcept> {
        self.concepts.get(id)
    }

    pub fn calculate_pnd_tax(&self, annual_income: Decimal, allowances: PersonalAllowances) -> Result<Decimal, String> {
        let mut taxable = annual_income;

        // Apply allowances
        taxable -= allowances.personal;
        if allowances.spouse { taxable -= dec!(60000); }
        taxable -= Decimal::from(allowances.children) * dec!(30000);
        taxable -= std::cmp::min(allowances.insurance, dec!(100000));
        taxable -= std::cmp::min(allowances.pension, dec!(500000));

        if taxable <= dec!(0) { return Ok(dec!(0)); }

        let mut tax = dec!(0);
        let mut remaining = taxable;

        for bracket in &self.tax_brackets_2026 {
            let bracket_income = match bracket.upper {
                Some(upper) => std::cmp::min(remaining, upper - bracket.lower + dec!(1)),
                None => remaining,
            };

            if bracket_income <= dec!(0) { break; }

            tax += bracket_income * bracket.rate;
            remaining -= bracket_income;

            if remaining <= dec!(0) { break; }
        }

        Ok(tax)
    }

    pub fn calculate_sso(&self, monthly_wage: Decimal) -> (Decimal, Decimal) {
        let wage_base = std::cmp::min(monthly_wage, self.sso_rules.max_wage_base);
        let employee_contribution = (wage_base * self.sso_rules.employee_rate).round_dp(2);
        let employer_contribution = (wage_base * self.sso_rules.employer_rate).round_dp(2);
        (employee_contribution, employer_contribution)
    }

    pub fn calculate_pf(&self, monthly_wage: Decimal, employee_rate: Decimal) -> Result<(Decimal, Decimal), String> {
        if employee_rate < self.pf_rules.min_employee_rate || employee_rate > self.pf_rules.max_employee_rate {
            return Err(format!("Employee rate must be between {}% and {}%",
                self.pf_rules.min_employee_rate * dec!(100),
                self.pf_rules.max_employee_rate * dec!(100)));
        }

        let employee_contribution = (monthly_wage * employee_rate).round_dp(2);
        let employer_contribution = (monthly_wage * self.pf_rules.employer_match_rate).round_dp(2);
        Ok((employee_contribution, employer_contribution))
    }

    pub fn calculate_severance(&self, last_monthly_wage: Decimal, days_worked: u32) -> Result<Decimal, String> {
        let years = days_worked / 365;
        let remaining_days = days_worked % 365;

        let severance_days = self.severance_rules.iter()
            .find(|rule| {
                years >= rule.min_days / 365 &&
                rule.max_days.map_or(true, |max| years < max / 365)
            })
            .map(|rule| rule.severance_days)
            .unwrap_or(0);

        let daily_wage = (last_monthly_wage / dec!(30)).round_dp(2);
        let severance_pay = (daily_wage * Decimal::from(severance_days)).round_dp(2);

        // Add pro-rated for partial year
        let pro_rated = (daily_wage * Decimal::from(remaining_days) / dec!(365)).round_dp(2);
        Ok(severance_pay + pro_rated)
    }

    pub fn calculate_ot(&self, hourly_rate: Decimal, weekday_hours: Decimal, weekend_hours: Decimal, holiday_hours: Decimal) -> Decimal {
        let weekday_pay = hourly_rate * self.ot_rules.weekday_rate * weekday_hours;
        let weekend_pay = hourly_rate * self.ot_rules.weekend_rate * weekend_hours;
        let holiday_pay = hourly_rate * self.ot_rules.holiday_rate * holiday_hours;
        (weekday_pay + weekend_pay + holiday_pay).round_dp(2)
    }

    pub fn validate(&self, data: &Value) -> Result<ValidationResult, String> {
        let mut errors = Vec::new();
        let mut warnings = Vec::new();

        // Validate employee data if present
        if let Some(emp) = data.get("employee") {
            if let Some(national_id) = emp.get("national_id").and_then(|v| v.as_str()) {
                if !regex::Regex::new(r"^[0-9]{13}$").unwrap().is_match(national_id) {
                    errors.push(ValidationError {
                        field: "employee.national_id".to_string(),
                        message: "National ID must be 13 digits".to_string(),
                        severity: ValidationSeverity::Error,
                    });
                }
            }

            if let Some(base_salary) = emp.get("base_salary").and_then(|v| v.as_str()).and_then(|s| s.parse::<Decimal>().ok()) {
                if base_salary < dec!(0) || base_salary > dec!(10000000) {
                    errors.push(ValidationError {
                        field: "employee.base_salary".to_string(),
                        message: "Base salary must be between 0 - 10,000,000 THB".to_string(),
                        severity: ValidationSeverity::Error,
                    });
                }
            }

            if let Some(hire_date) = emp.get("hire_date").and_then(|v| v.as_str()) {
                if let Ok(date) = NaiveDate::parse_from_str(hire_date, "%Y-%m-%d") {
                    if date > chrono::Utc::now().date_naive() {
                        errors.push(ValidationError {
                            field: "employee.hire_date".to_string(),
                            message: "Hire date cannot be in the future".to_string(),
                            severity: ValidationSeverity::Error,
                        });
                    }
                }
            }
        }

        Ok(ValidationResult {
            valid: errors.is_empty(),
            errors,
            warnings,
        })
    }
}

// Supporting types

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaxBracket {
    pub lower: Decimal,
    pub upper: Option<Decimal>,
    pub rate: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SSORules {
    pub employee_rate: Decimal,
    pub employer_rate: Decimal,
    pub min_wage_base: Decimal,
    pub max_wage_base: Decimal,
}

impl Default for SSORules {
    fn default() -> Self {
        Self {
            employee_rate: dec!(0.05),
            employer_rate: dec!(0.05),
            min_wage_base: dec!(1650),
            max_wage_base: dec!(15000),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PFRules {
    pub min_employee_rate: Decimal,
    pub max_employee_rate: Decimal,
    pub employer_match_rate: Decimal,
}

impl Default for PFRules {
    fn default() -> Self {
        Self {
            min_employee_rate: dec!(0.02),
            max_employee_rate: dec!(0.15),
            employer_match_rate: dec!(0.05),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeveranceRule {
    pub min_days: u32,
    pub max_days: Option<u32>,
    pub severance_days: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OTRules {
    pub weekday_rate: Decimal,
    pub weekend_rate: Decimal,
    pub holiday_rate: Decimal,
    pub max_hours_per_week: Decimal,
}

impl Default for OTRules {
    fn default() -> Self {
        Self {
            weekday_rate: dec!(1.5),
            weekend_rate: dec!(2.0),
            holiday_rate: dec!(3.0),
            max_hours_per_week: dec!(36),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeaveType {
    pub code: String,
    pub label_th: String,
    pub label_en: String,
    pub max_days: u32,
    pub carry_forward: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonalAllowances {
    pub personal: Decimal,
    pub spouse: bool,
    pub children: u32,
    pub insurance: Decimal,
    pub pension: Decimal,
}

impl Default for PersonalAllowances {
    fn default() -> Self {
        Self {
            personal: dec!(60000),
            spouse: false,
            children: 0,
            insurance: dec!(0),
            pension: dec!(0),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    pub valid: bool,
    pub errors: Vec<ValidationError>,
    pub warnings: Vec<ValidationWarning>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationError {
    pub field: String,
    pub message: String,
    pub severity: ValidationSeverity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationWarning {
    pub field: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ValidationSeverity {
    Error,
    Warning,
    Info,
}