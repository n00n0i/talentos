-- PF membership is not SSO enrolment. Every employee covered by Social Security
-- Act §33 pays SSO regardless of the provident fund. Default TRUE (Thai payroll
-- norm); HR unchecks it for exempt cases (e.g. some foreign staff).
ALTER TABLE employees ADD COLUMN sso_enrolled BOOLEAN NOT NULL DEFAULT TRUE;
