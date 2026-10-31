# TalentOS — Handoff

อัปเดต: 2026-10-31  
Repo: `/home/opc/content-creator/talentos`  
Branch: `master` @ `ce88371`  
CI: GitHub Actions `CI` เขียว (quality / web / e2e / build-release) — Strix Pentest อาจยังรันแยกอยู่

---

## 1. ภาพรวม

TalentOS คือ SaaS HR + เงินเดือนสำหรับองค์กรไทย (Thai-first):

- Rust workspace: Axum gateway, payroll engine, HR services, GraphQL/MCP, PostgreSQL + RLS
- Next.js App Router: Thai UI, cookie session ผ่าน Next proxy
- เงินเป็น `rust_decimal`; payroll เป็น deterministic DAG
- Tenant isolation ด้วย PostgreSQL RLS (`talentos_app` เป็น `NOBYPASSRLS`)

สถานะทดสอบล่าสุด: **451 Rust tests passed, 1 ignored (load)** และ **53/53 Playwright**  
Migrations: **63 ไฟล์** (`sqlx migrate run` บน DB ใหม่ผ่าน)

---

## 2. โครงสร้างโค้ด

```
crates/
  core/            authz (RBAC matrix), kernel (TenantContext), ...
  domain/          payroll/attendance domain rules
  infra/db         TenantPool, begin_tenant, PII cipher
  services/
    talentos-hr-runner       HR: attendance, leave, OT, claims, loans, documents,
                             onboarding, recruitment, training, performance, privacy, ...
    talentos-payroll-runner  payroll run, PDF, load test
  api/
    gateway          REST /api/v1, auth, setup, metrics
    graphql, mcp
web/               Next.js app (Thai UI), API proxy, E2E
migrations/        SQLx migrations (63)
scripts/           restore-drill.sh, ...
docs/              BUILD_STATUS, QA, ops, plans
```

### หน้าหลักในเว็บ

`/setup`, `/login`, `/`, `/employees`, `/attendance`, `/calendar`, `/clock`, `/leave`,
`/ot`, `/claims`, `/payroll`, `/payslip`, `/me/payslips`, `/reports`, `/loans`,
`/final-settlement`, `/users`, `/settings`, `/forms`, `/audit`, `/privacy`,
`/documents`, `/training`, `/onboarding`, `/recruitment`, `/performance`,
`/goals`, `/me/reviews`

---

## 3. ฟีเจอร์ที่ทำเสร็จแล้ว (ล่าสุด)

- Attendance: กะ, geofence, import, correction/approval, team calendar
- Offline PWA: คิวลงเวลาใน `localStorage`, `client_id` idempotent, คงเวลาเดิม (`client_time` ≤ 7 วัน), service worker แคชเฉพาะ navigation
- Payroll: รอบคำนวณ, anomaly review gate ก่อนอนุมัติ, bank payout CSV configurable, mark-paid, payslip/50 ทวิ PDF
- Leave/OT/claims/loans/PF/GL/reports
- PDPA: `/privacy`, consent version, DSAR JSON export (account, employee ทุกคนที่ผูก, attendance, leave, OT, claims, loans, payslips, documents, onboarding, training, reviews, goals)
- Documents: 1 ไฟล์/เอกสาร, expiry 60 วัน
- Metrics: `GET /metrics` (2xx/4xx/5xx/other, duration sum/count, uptime), optional `METRICS_TOKEN`
- Onboarding: checklist มาตรฐาน 6 งานจากวันเริ่มงาน + งานเอง
- Recruitment: job openings, candidates, pipeline, CV 1 ไฟล์, stage history, interview scheduling, offer letter PDF + status
- Training: ประวัติอบรม + certificate expiry 90 วัน
- Performance: review cycles, ratings/feedback, employee `/me/reviews` acknowledgement, goals/OKR
- Ops: `scripts/restore-drill.sh`, CI restore drill, CI payroll load 600 employees

---

## 4. รัน local

### 4.1 Containers

- Postgres: `loans-postgres-1` (volume `loans_pgdata`)
- Redis: `talentos-redis-1` (password `redis_dev_secret`)

### 4.2 Migrations

Production/CI ใช้:

```sh
sqlx migrate run
```

Dev DB เคย apply บางไฟล์ด้วยมือและบันทึก `_sqlx_migrations` เอง — ถ้าเพิ่ม migration ใหม่บน dev ให้ apply ด้วย `psql` แล้ว insert แถว `_sqlx_migrations` (version, description, success, checksum sha384, execution_time) หรือใช้ `sqlx migrate run` ถ้า checksum ตรงทั้งหมด

### 4.3 Gateway

```sh
cd /home/opc/content-creator/talentos
export DATABASE_URL="postgres://talentos_app:app_dev_secret@localhost:5432/talentos"
export DATABASE_AUTH_URL="postgres://talentos_auth:auth_dev_secret@localhost:5432/talentos"
export DATABASE_ADMIN_URL="postgres://talentos:talentos_dev_secret@localhost:5432/talentos"
export REDIS_URL="redis://:redis_dev_secret@127.0.0.1:6379"
export AUTH_JWT_SECRET="dev-secret-change-me-in-prod-minimum-32-bytes-okay!!"
export GATEWAY_PORT=18479
export PAYSLIP_STORE_DIR=/tmp/opencode/payslips
export DOCUMENT_UPLOAD_DIR=/tmp/opencode/documents
export CANDIDATE_CV_DIR=/tmp/opencode/cvs
export PII_ENCRYPTION_KEY="aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
export SETUP_TOKEN=e2e-setup-token
export TALENTOS_COOKIE_SECURE=false
cargo build -p talentos-gateway
nohup ./target/debug/talentos-gateway > /tmp/opencode/gateway.log 2>&1 &
curl -sf http://127.0.0.1:18479/health
```

`PII_ENCRYPTION_KEY` ตัวอย่างเป็น test key 64 hex — production ต้องสุ่มจริงและเก็บแยกจาก DB  
`SETUP_TOKEN` ใช้สร้าง tenant เพิ่มได้เฉพาะ non-production; production อนุญาต first-run เท่านั้น

### 4.4 Web

```sh
cd web
npm ci                       # ครั้งแรก
npm run build
rm -rf .next/standalone/.next/static .next/standalone/public
cp -r .next/static .next/standalone/.next/
cp -r public .next/standalone/
cd .next/standalone
PORT=3156 HOSTNAME=127.0.0.1 \
TALENTOS_API_URL=http://127.0.0.1:18479 \
TALENTOS_COOKIE_SECURE=false \
node server.js
```

หมายเหตุ: ต้อง copy static/public เข้า standalone เอง ไม่งั้นหน้าเว็บจะ 400/500 เรื่อง chunk

### 4.5 E2E

E2E ต้องเริ่มจาก **DB ว่าง** และสร้างทุกอย่างผ่าน UI (ไม่มี seed SQL):

```sh
docker exec loans-postgres-1 psql -U talentos -d talentos -c "DELETE FROM tenants;"
cd web
E2E_BASE_URL=http://127.0.0.1:3156 \
E2E_SETUP_TOKEN=e2e-setup-token \
npx playwright test
```

- `app.spec.ts` เป็น serial และใช้ tenant ร่วมกัน
- `roles.spec.ts` สร้าง tenant แยกด้วย `SETUP_TOKEN`
- Login แรกสร้าง tenant/admin ที่ `/setup` (slug `e2e`)

---

## 5. คำสั่งคุณภาพ

```sh
cargo fmt --all -- --check
cargo clippy -p talentos-hr-runner -p talentos-gateway --all-targets -- -D warnings
cargo test --workspace
```

ต้อง export `DATABASE_URL`, `DATABASE_AUTH_URL`, `DATABASE_ADMIN_URL` ก่อนรัน test ที่ใช้ DB  
ถ้าแก้ `query!`/`query_as!` ให้รัน `cargo sqlx prepare --workspace -- --all-targets`

Load test (ไม่รันใน suite ปกติ):

```sh
LOAD_EMPLOYEES=600 \
DATABASE_URL=postgres://talentos_app:app_dev_secret@localhost:5432/talentos \
DATABASE_ADMIN_URL=postgres://talentos:talentos_dev_secret@localhost:5432/talentos \
cargo test -p talentos-payroll-runner --test load -- --ignored --nocapture
```

Restore drill:

```sh
# destination DB ต้องมีอยู่ก่อน
createdb -h localhost -U talentos talentos_restore   # หรือ psql -c "CREATE DATABASE ..."
SRC_DATABASE_URL=postgres://talentos:talentos_dev_secret@localhost:5432/talentos \
DEST_DATABASE_URL=postgres://talentos:talentos_dev_secret@localhost:5432/talentos_restore \
bash scripts/restore-drill.sh
```

บนเครื่อง dev นี้ไม่มี `psql`/`pg_dump` ที่ host — รันผ่าน container ได้:

```sh
docker cp scripts/restore-drill.sh loans-postgres-1:/tmp/restore-drill.sh
docker exec loans-postgres-1 psql -U talentos -d postgres -c "CREATE DATABASE talentos_restore"
docker exec -e SRC_DATABASE_URL="postgres://talentos:talentos_dev_secret@localhost:5432/talentos" \
  -e DEST_DATABASE_URL="postgres://talentos:talentos_dev_secret@localhost:5432/talentos_restore" \
  loans-postgres-1 bash /tmp/restore-drill.sh
```

---

## 6. ข้อตกลงสำคัญของโค้ด

- **RLS:** ทุก service ใช้ `pool.begin_tenant(tenant_id)` และ query มี `tenant_id = $1` ชัดเจน; อย่าพึ่ง RLS อย่างเดียว
- **Money:** `rust_decimal::Decimal` เท่านั้น; อย่าใช้ float
- **ห้าม `unwrap()`/`expect()` ใน production code** (test ได้)
- **Audit:** การกระทำสำคัญเขียน `audit_log` (เช่น payroll, privacy export, attendance)
- **Permissions:** แก้ที่ `crates/core/authz/src/rbac.rs`; UI ใช้ `me.permissions` เพื่อซ่อนปุ่ม
- **PII:** `national_id`, `bank_account_number`, `sso_number` เข้ารหัสด้วย `PiiCipher`; legacy plaintext ผ่าน decrypt ได้
- **Migration:** เพิ่มไฟล์ใหม่ + apply + บันทึก `_sqlx_migrations` บน dev; CI ใช้ fresh DB
- **Next proxy:** `web/app/api/gw/[...path]/route.ts` ต้องมี HTTP method ครบ (เคยขาด `PATCH`); body limit สำหรับ upload ตั้งที่ router
- **UI:** ภาษาไทย, `handleApiError`, `data-testid` สำหรับ E2E, `canWrite = me.permissions.includes("employee.update")` สำหรับหน้าที่ payroll ดูได้แต่แก้ไม่ได้

---

## 7. Storage / Env ที่อยู่นอก DB

| ข้อมูล | Env | Default dev |
|---|---|---|
| Payslip PDF | `PAYSLIP_STORE_DIR` | `/tmp/opencode/payslips` |
| Claim receipts | `CLAIM_UPLOAD_DIR` | `/tmp/talentos-claims` |
| Employee documents | `DOCUMENT_UPLOAD_DIR` | `/tmp/talentos-documents` |
| Candidate CV | `CANDIDATE_CV_DIR` | `/tmp/talentos-cvs` |
| Metrics auth | `METRICS_TOKEN` | ไม่ตั้ง = เปิด |
| Chromium PDF | `CHROME_BIN` | `chromium-browser` |

ทุกไฟล์ต้อง mount volume/backup ก่อน production; dump DB ไม่มีไฟล์เหล่านี้

---

## 8. งานที่ยังเหลือ / ติด external

1. **SSO (OIDC)** — ต้องมี OAuth client credentials; ยังไม่ได้ทำ generic OIDC
2. **PDPA deletion/anonymisation + retention purge** — ต้องตัดสินนโยบายเก็บรักษา (ภาษี/ค่าจ้างตามกฎหมาย)
3. **E-signature provider** — offer letter เป็น PDF/สถานะแล้ว แต่ยังไม่มีลายเซ็นถูกต้องตามกฎหมาย
4. **Performance ต่อ** — calibration, manager workflow (ยังไม่มี reporting line ใน schema), goal check-ins
5. **Ops ต่อ** — business gauges (notification backlog/payroll), alert receiver, WAL/PITR, scheduled backup
6. **EN UI / i18n**
7. **Phase 0 official formats** — bank layouts จริง, e-filing, template 50 ทวิ/สปส. ต้องมีตัวอย่างและ accountant sign-off
8. **Candidate PII at rest** — email/phone ของผู้สมัครยังเป็น plaintext (พนักงานใช้ PII cipher)

---

## 9. ความเสี่ยงที่ต้องรู้

- `PII_ENCRYPTION_KEY` หาย = ถอดข้อมูลที่เข้ารหัสไม่ได้; backup key แยกจาก DB
- ไฟล์ upload อยู่ local disk; default `/tmp` หายเมื่อ reboot
- Bank payout CSV เป็น template configurable; ยังไม่ตรง format ธนาคารจริง และช่องบัญชีว่างได้ถ้าพนักงานไม่ได้กรอก
- `mark-paid` ไม่มี undo
- Anomaly review gate ใช้ `payroll_run.approve` (ไม่มี permission แยก); backend กัน 409 แต่ UI อาจ enable ก่อน panel โหลด
- Offline queue: error 5xx/401 เก็บไว้ลองใหม่, 4xx เชิงธุรกิจถูก drop; `client_time` ยอมให้ลงเวลาย้อน ≤7 วัน (มี audit)
- Performance: HR ส่งผลใหม่จะล้าง acknowledgement; ไม่มีช่องโต้แย้ง
- Offer PDF ต้องมี Chromium; ถ้าไม่มีจะ 503 (E2E ตรวจแค่ลิงก์ PDF เพราะ CI gateway ไม่มี Chromium)
- E2E เป็น stateful/serial; ต้องล้าง tenant ก่อนทุกครั้ง
- Dev DB `_sqlx_migrations` อาจไม่ตรง checksum กับไฟล์ทุกตัว; CI fresh DB คือแหล่งความจริง

---

## 10. จุดเริ่มต้นแนะนำสำหรับคนรับงาน

1. อ่าน `docs/BUILD_STATUS.md` และ `docs/qa/e2e-journeys.md`
2. รัน `cargo test --workspace` + E2E ตามด้านบน เพื่อยืนยัน baseline 451/53
3. ถ้ามี OAuth credentials: เริ่ม SSO generic OIDC (discovery, PKCE, per-tenant config, link by email)
4. ถ้ายังไม่มี: ทำ EN UI/i18n หรือ performance manager workflow (ต้องเพิ่ม reporting line) หรือ ops business gauges
5. ก่อน production: ตัดสิน PDPA retention, ตั้ง volume/backup, เปลี่ยน secrets, ตรวจ bank/e-filing กับ accountant

---

## 11. เอกสาร/ไฟล์สำคัญ

- `docs/BUILD_STATUS.md` — สถานะรวม, test counts, known gaps
- `docs/qa/e2e-journeys.md` — E2E coverage matrix
- `docs/ops/metrics.md` — scrape/alert
- `docs/ops/backup-restore.md` — backup/drill/load
- `docs/superpowers/plans/` — plan ล่าสุดของแต่ละฟีเจอร์
- `crates/api/gateway/src/router.rs` — routes + body limits
- `crates/core/authz/src/rbac.rs` — permission matrix
- `crates/services/talentos-hr-runner/src/privacy.rs` — DSAR export
- `web/app/api/gw/[...path]/route.ts` — session proxy/refresh
- `web/e2e/app.spec.ts`, `web/e2e/roles.spec.ts` — real-user E2E

---

## 12. Gotchas

- Gateway ไม่ได้ serve static web; ต้องรันแยกและตั้ง `TALENTOS_API_URL`
- Next standalone ต้อง copy `.next/static` + `public` เข้า `.next/standalone`
- ถ้าเพิ่ม route/method ใหม่ ต้องเพิ่มใน Next proxy ด้วย
- `cargo test` บางชุด skip ถ้าไม่ตั้ง `DATABASE_URL`; อย่าคิดว่าผ่านครบถ้าไม่ได้ตั้ง env
- PDF tests จะ skip ถ้าไม่มี Chromium
- หลัง merge งานใหม่ อย่าลืมอัปเดต `docs/BUILD_STATUS.md` (tests/E2E/migrations) และ README ถ้ามีข้อจำกัดใหม่
