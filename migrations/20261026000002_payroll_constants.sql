CREATE TABLE tax_brackets (
    id SERIAL PRIMARY KEY,
    tenant_id UUID REFERENCES tenants(id) ON DELETE SET NULL,
    year INT NOT NULL,
    lower NUMERIC(12,2) NOT NULL,
    upper NUMERIC(12,2),
    rate NUMERIC(5,4) NOT NULL,
    UNIQUE(tenant_id, year, lower)
);

INSERT INTO tax_brackets (tenant_id, year, lower, upper, rate) VALUES
(NULL, 2026, 0, 150000, 0.0000),
(NULL, 2026, 150000.01, 300000, 0.0500),
(NULL, 2026, 300000.01, 500000, 0.1000),
(NULL, 2026, 500000.01, 750000, 0.1500),
(NULL, 2026, 750000.01, 1000000, 0.2000),
(NULL, 2026, 1000000.01, 2000000, 0.2500),
(NULL, 2026, 2000000.01, NULL, 0.3000);

CREATE TABLE sso_rules (
    id SERIAL PRIMARY KEY,
    tenant_id UUID REFERENCES tenants(id) ON DELETE SET NULL,
    rate NUMERIC(5,4) NOT NULL DEFAULT 0.0500,
    max_wage_base NUMERIC(12,2) NOT NULL DEFAULT 15000,
    effective_from DATE NOT NULL DEFAULT '2026-01-01'
);

INSERT INTO sso_rules (tenant_id, rate, max_wage_base, effective_from) VALUES (NULL, 0.0500, 15000, '2026-01-01');

CREATE TABLE pf_rules (
    id SERIAL PRIMARY KEY,
    tenant_id UUID REFERENCES tenants(id) ON DELETE SET NULL,
    employee_rate NUMERIC(5,4) NOT NULL DEFAULT 0.0500,
    employer_rate NUMERIC(5,4) NOT NULL DEFAULT 0.0500,
    max_base NUMERIC(12,2) NOT NULL DEFAULT 15000,
    effective_from DATE NOT NULL DEFAULT '2026-01-01'
);

INSERT INTO pf_rules (tenant_id, employee_rate, employer_rate, max_base, effective_from) VALUES (NULL, 0.0500, 0.0500, 15000, '2026-01-01');

CREATE TABLE severance_rules (
    id SERIAL PRIMARY KEY,
    tenant_id UUID REFERENCES tenants(id) ON DELETE SET NULL,
    min_months INT NOT NULL,
    max_months INT,
    days INT NOT NULL,
    UNIQUE(tenant_id, min_months)
);

INSERT INTO severance_rules (tenant_id, min_months, max_months, days) VALUES
(NULL, 3, 6, 30),
(NULL, 6, 12, 90),
(NULL, 12, 36, 90),
(NULL, 36, 72, 180),
(NULL, 72, 120, 240),
(NULL, 120, 240, 300),
(NULL, 240, NULL, 400);

CREATE TABLE ot_rules (
    id SERIAL PRIMARY KEY,
    tenant_id UUID REFERENCES tenants(id) ON DELETE SET NULL,
    weekday_rate NUMERIC(4,2) NOT NULL DEFAULT 1.50,
    weekend_rate NUMERIC(4,2) NOT NULL DEFAULT 2.00,
    holiday_rate NUMERIC(4,2) NOT NULL DEFAULT 3.00,
    max_weekly_hours NUMERIC(5,2) NOT NULL DEFAULT 36.00,
    effective_from DATE NOT NULL DEFAULT '2026-01-01'
);

INSERT INTO ot_rules (tenant_id, weekday_rate, weekend_rate, holiday_rate, max_weekly_hours, effective_from) VALUES (NULL, 1.50, 2.00, 3.00, 36.00, '2026-01-01');