-- Employer SSO account/branch fields and ภ.ง.ด.1 / ภ.ง.ด.1ก statutory templates.
-- entities gains the real SSO employer account and branch number; the สปส.1-10
-- default header switches from empty placeholders to those columns, and the two
-- withholding forms get editable global default templates.

ALTER TABLE entities ADD COLUMN sso_account TEXT;
ALTER TABLE entities ADD COLUMN branch TEXT NOT NULL DEFAULT '000000';

-- Extend form_type to include the monthly (ภ.ง.ด.1) and annual (ภ.ง.ด.1ก) forms.
ALTER TABLE form_templates DROP CONSTRAINT form_templates_form_type_check;
ALTER TABLE form_templates ADD CONSTRAINT form_templates_form_type_check
    CHECK (form_type IN ('50tawi', 'sso1_10', 'pnd1', 'pnd1a'));

-- สปส.1-10 default header now sources the real SSO account / branch.
UPDATE form_templates
   SET config = $${
    "title": "แบบรายการแสดงการส่งเงินสมทบ",
    "header": [
      {"key":"employer_name","label":"ชื่อสถานประกอบการ","source":"entity_name"},
      {"key":"employer_account","label":"เลขที่บัญชีนายจ้าง","source":"sso_account"},
      {"key":"branch","label":"ลำดับที่สาขา","source":"branch"}
    ],
    "columns": [
      {"label":"เลขประจำตัวประชาชน","source":"national_id"},
      {"label":"ชื่อ-สกุล","source":"employee_name"},
      {"label":"ค่าจ้าง","source":"gross"},
      {"label":"เงินสมทบลูกจ้าง","source":"sso_employee"},
      {"label":"เงินสมทบนายจ้าง","source":"sso_employer"}
    ]
  }$$::jsonb
 WHERE tenant_id IS NULL AND form_type = 'sso1_10' AND is_active;

INSERT INTO form_templates (tenant_id, form_type, version, config, is_active)
VALUES
  (NULL, 'pnd1', 1, $${
    "title": "ภ.ง.ด.1",
    "header": [
      {"key":"payer_name","label":"ผู้จ่ายเงิน","source":"entity_name"},
      {"key":"payer_tax_id","label":"เลขประจำตัวผู้เสียภาษีผู้จ่าย","source":"entity_tax_id"},
      {"key":"period_start","label":"รอบเงินเดือนเริ่ม","source":"period_start"},
      {"key":"period_end","label":"รอบเงินเดือนสิ้น","source":"period_end"}
    ],
    "columns": [
      {"label":"รหัสพนักงาน","source":"employee_code"},
      {"label":"ชื่อ-สกุล","source":"employee_name"},
      {"label":"เลขประจำตัวประชาชน","source":"national_id"},
      {"label":"เงินได้","source":"gross"},
      {"label":"ภาษีหัก ณ ที่จ่าย","source":"tax"}
    ]
  }$$::jsonb, TRUE),
  (NULL, 'pnd1a', 1, $${
    "title": "ภ.ง.ด.1ก",
    "header": [
      {"key":"payer_name","label":"ผู้จ่ายเงิน","source":"entity_name"},
      {"key":"payer_tax_id","label":"เลขประจำตัวผู้เสียภาษีผู้จ่าย","source":"entity_tax_id"},
      {"key":"year","label":"ปีภาษี","source":"year"}
    ],
    "columns": [
      {"label":"รหัสพนักงาน","source":"employee_code"},
      {"label":"ชื่อ-สกุล","source":"employee_name"},
      {"label":"เลขประจำตัวประชาชน","source":"national_id"},
      {"label":"เงินได้ทั้งปี","source":"annual_income"},
      {"label":"ภาษีหัก ณ ที่จ่ายทั้งปี","source":"annual_tax"}
    ]
  }$$::jsonb, TRUE);
