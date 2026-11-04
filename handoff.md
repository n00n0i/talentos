# TalentOS — Handoff

อัปเดต: 2026-11-04 (QA pass)
Repo: `/home/opc/content-creator/talentos`
Branch: `master` (QA pass บน `2a2382b`)
Migrations: 72 ไฟล์
สถานะทดสอบล่าสุด: **Playwright 63/63 passed** (clean DB, desktop+mobile route crawl) · `cargo test --workspace` **534 passed / 1 ignored** · `tsc` 0 · `vitest` 41/41 · `next build` ผ่าน

QA 2026-11-04: เพิ่ม route/UX crawl สำหรับทุก role, แก้ active nav ของหน้าย่อย, table overflow, ฟอร์มมือถือ, คำผิดภาษาไทย (2FA/OIDC/ลายมือชื่อ), `html lang` ตอนสลับ EN และเพิ่มเทสต์ mobile drawer/menu search

---

## 1. ภาพรวม

TalentOS = SaaS HR + เงินเดือนสำหรับองค์กรไทย (Thai-first)

- **Rust workspace**: Axum gateway, payroll engine, HR services, GraphQL/MCP, PostgreSQL + RLS
- **Next.js 15 App Router**: Thai UI + EN toggle, cookie session ผ่าน Next proxy (`/api/session/*`, `/api/gw/*`)
- เงินใช้ `rust_decimal`; payroll เป็น deterministic DAG
- Tenant isolation ด้วย PostgreSQL RLS (`talentos_app` = `NOBYPASSRLS`)

---

## 2. โครงสร้างโค้ด

```
crates/
  core/        authz (RBAC matrix), kernel (TenantContext)
  domain/      payroll/attendance domain rules
  infra/db     TenantPool, begin_tenant, PII cipher
  services/
    talentos-hr-runner      attendance, leave, OT, claims, loans, documents,
                            onboarding, recruitment, training, performance,
                            goals, privacy, kg, kg_sync
    talentos-payroll-runner payroll run, PDF
  ai/
    talentos-model-gw       OpenAI-compatible client (chat, embeddings, cost)
    talentos-rag            chunking + pgvector search
    talentos-agent          Observe→Plan→Act→Reflect runner
    talentos-tools          tool registry (calculate_tax, check_compliance, …)
    talentos-memory         memory layers + recall
  api/
    gateway                 REST /api/v1, auth, setup, metrics, secrets (OpenBao)
    graphql, mcp
web/                        Next.js app + E2E
migrations/                 71 SQLx migrations
scripts/                    deploy/backup/bootstrap/openbao-bootstrap
docs/superpowers/specs|plans  ดีไซน์ + แผนงานทั้งหมด
```

---

## 3. ทำอะไรได้แล้ว (feature inventory)

### 3.1 ระบบหลัก
- Login/logout, refresh token rotation + reuse detection, 2FA (TOTP), invite/reset flows, SSO/OIDC (JWKS verify)
- Multi-tenant setup ครั้งแรก (`/setup` + setup token), tenant/entity สลับได้ตามสิทธิ์
- RBAC 4 บทบาท: `hr_admin`, `payroll_manager`, `manager`, `employee` + delegation

### 3.2 HR
- พนักงาน (CRUD, CSV import, manager line, cost center, PF rates)
- เอกสารพนักงาน + ไฟล์ + วันหมดอายุ (`/documents/expiring`)
- Onboarding checklist, Recruitment pipeline, Training records
- Performance: รอบประเมิน, review, calibration, employee acknowledge
- Goals/OKR + progress
- Attendance: ลงเวลา, กะ, วันหยุด, นำเข้า CSV, อนุมัติ/แก้ไข
- Leave (หลายประเภท, โควตา, 2-step approval, delegation), OT, Claims (พร้อมใบเสร็จ)
- Loans, Final settlement, Privacy/PDPA (retention + purge)

### 3.3 Payroll
- สร้าง/คำนวณ/อนุมัติ/จ่ายรอบ, สลิป PDF, 50 ทวี, PND.1, สปส., กองทุนสำรองฯ
- Anomaly review, bank transfer file, template สลิป, payroll constants ที่ตั้งได้

### 3.4 Dashboard & UI
- Shell ใหม่: sidebar แบ่งกลุ่ม + command bar + drawer มือถือ + ค้นหาเมนู
- Dashboard ตามบทบาท (HR / payroll / manager / employee) พร้อม KPI, งานรออนุมัติ, งานติดตาม, กราฟ OT, สรุปงวด, donut แผนก, กิจกรรม
- Reports: headcount, payroll cost, OT trend
- Form control ทั้งแอปเป็นระบบเดียว (input/select/checkbox/date/radius/สี)
- Chat head มุมขวาล่างทุกหน้า

### 3.5 AI (Phase 4 — บางส่วน)
- **Providers**: เพิ่มได้หลายเจ้า (OpenAI-compatible presets: OpenAI, OpenRouter, Groq, Together, Mistral, DeepSeek, xAI, Ollama, vLLM, LM Studio, custom) + **ดึงรายการโมเดล** จาก provider
- **Model roles**: chat / embedding / ocr / vision / graph_extract
- **MCP / Skills / Agents**: มี UI ตั้งค่า (MCP toggle, skills จาก tool registry, agents list) — ยังไม่ใช่ของที่ผู้ใช้ "สร้างเอง" เต็มรูปแบบ
- **Knowledge graph องค์กร**: 12 คลาส (Organization, LegalEntity, Department, Position, Employee, Skill, Policy, Benefit, PayrollItem, Cycle, Document, Process) + ความสัมพันธ์ตาม taxonomy, rebuild ได้, ดู neighbours ได้
- **Chat API** `POST /api/v1/ai/chat` — grounded ด้วย subgraph + citations + cost tracking
- **Secrets**: provider key เก็บใน **OpenBao** (KV v2, auto-unseal) — ไม่ลง Postgres

---

## 4. Feature ที่สเปกกำหนดแต่ยังไม่มี

อ้างอิง `docs/design/phase-4-spec.md`:

| รายการ | สถานะ |
|---|---|
| `POST /ai/agents/:id/status` | ❌ |
| `POST /ai/agents/:id/approve` (human-in-the-loop signal) | ❌ (มี `allow_destructive` แต่ไม่มี approval flow) |
| `POST /ai/documents` + `/ai/documents/search` (RAG ingest + hybrid search + citations) | ❌ (มีตาราง embeddings แต่ไม่มี API) |
| `GET /ai/cost` (รายงานต้นทุน) | ❌ (บันทึก cost แล้ว แต่ไม่มี endpoint/หน้า) |
| Temporal workflow สำหรับ agent | ❌ (runner เป็น in-process) |
| MCP server ที่ผู้ใช้เพิ่มเอง + tool sync | ❌ (ศึกษาไว้ใน `2026-11-03-ai-providers-mcp-skills-agents-study.md`) |
| Skills ที่ผู้ใช้สร้าง + versioning | ❌ |
| Agents ที่ผู้ใช้สร้าง + ผูก skills/tools | ❌ |

---

## 5. สิ่งที่เหลือต้องทำ (แนะนำลำดับ)

### P0 — ยังไม่ครบตามดีไซน์ AI ล่าสุด
1. **Provider ให้ถูกจริง** — `auth_style` (bearer / x-api-key / query / none), `catalog_style` (openai / anthropic / google / ollama), `chat_style`, capability override, ปุ่มทดสอบ provider
2. **MCP servers ที่ผู้ใช้เพิ่มได้** — transport stdio / sse / streamable-http, sync `tools/list`, toggle ราย tool, namespace `mcp:<server>:<tool>`, secret ใน OpenBao
3. **Skills ที่ผู้ใช้สร้าง** — prompt + tool allowlist + model role + KG scope + versioning
4. **Agents ที่ผู้ใช้สร้าง** — ผูก model role + skills + tools + kg scope, `chat` รับ `agent_id`

### P1 — ปิดช่องว่าง Phase 4
5. `/ai/documents` + `/ai/documents/search` (RAG + citations)
6. `/ai/agents/:id/status` + `/approve` (human gate)
7. `/ai/cost` + หน้า dashboard ต้นทุน
8. Audit row `ai.settings.update`

### P2 — คุณภาพ/KG
9. KG: prune node ที่หายไป, populate Benefit/Process, embedding nodes เมื่อมี key
10. Retrieval ภาษาไทยไม่เว้นวรรค (ตอนนี้ lexical match คำตรงตัว)
11. KG inspector: graph visualization

### P3 — Ops
12. Push `master` (63 commits) + tag
13. จัดการ **migration drift** (`20261101000003` ถูกแก้หลัง apply → `sqlx migrate run` จะหยุด)
14. CI: เพิ่ม job สำหรับ AI/KG tests
15. แยก DB สำหรับ E2E เพื่อไม่ให้ `DELETE FROM tenants` ลบข้อมูล demo

---

## 6. วิธีรัน / ทดสอบ

### Services (systemd แล้ว — อยู่ถาวรข้าม reboot)
```bash
systemctl status talentos-gateway.service talentos-web.service
sudo systemctl restart talentos-web.service     # หลัง build
sudo journalctl -u talentos-web.service -n 50 --no-pager
```
- Gateway `:18479` (env ที่ `/etc/talentos-gateway.env`, token OpenBao อ่านจาก `/etc/openbao/ai.token`)
- Web `:3000` (standalone build ที่ `web/.next/standalone`)
- OpenBao `:8200` (docker `openbao`, auto-unseal จาก `/etc/openbao/seal.key`)

### Deploy build ใหม่
```bash
cd web && npm run build
rm -rf .next/standalone/.next/static .next/standalone/public
cp -r .next/static .next/standalone/.next/ && cp -r public .next/standalone/
sudo systemctl restart talentos-web.service
```

### Migrations
```bash
DATABASE_URL="postgres://talentos:talentos_dev_secret@localhost:5432/talentos" sqlx migrate run
```

### Tests
```bash
cd crates && cargo test -p talentos-gateway -p talentos-hr-runner
cd web && npx tsc --noEmit && npx vitest run && npm run build
docker exec loans-postgres-1 psql -U talentos -d talentos -c "DELETE FROM tenants;"
E2E_BASE_URL=http://127.0.0.1:3000 E2E_SETUP_TOKEN=e2e-setup-token npx playwright test
```
> ⚠️ E2E ล้าง tenant ทั้งหมด — หลังรันต้องสร้าง demo กลับ (ดู §7)

### Demo tenant
```
http://<host>:3000   องค์กร: demo
admin@demo.test / DemoPass12345   (สร้างด้วย setup token e2e-setup-token)
```

---

## 7. สร้าง demo tenant กลับหลัง E2E

```bash
curl -s -X POST -H "content-type: application/json" \
 -d '{"org_name":"Demo Co","slug":"demo","entity_name":"Demo HQ",
      "admin_email":"admin@demo.test","password":"DemoPass12345",
      "setup_token":"e2e-setup-token"}' \
 http://127.0.0.1:3000/api/gw/api/v1/setup
```

---

## 8. ข้อควรระวัง / ปัญหาที่รู้อยู่

1. **Migration drift** — `20261101000003` ถูกแก้หลัง apply ไปแล้ว ทำให้ `sqlx migrate run` หยุด; migration `20261216000001_ai_providers` ถูก apply + ลงทะเบียนมือไปแล้ว ต้องจัดการ drift ก่อน deploy ใหม่
2. **`Requires=` ของ web → gateway** เปลี่ยนเป็น `Wants=` แล้ว (ไม่งั้น gateway restart จะลาก web ล่ม)
3. **SELinux Enforcing** — systemd รัน node จาก `/home` ไม่ได้ → ใช้ `/usr/local/bin/talentos-node` ผ่าน `sh -c`
4. **AI provider ยังไม่ตั้งค่า** — `api_key_set: false`; chat ตอบ 503 จนกว่าจะใส่ key ที่ `/settings/ai`
5. **E2E ลบ tenant ทั้งหมด** — ต้อง seed demo กลับ (ดู §7)
6. **KG retrieval เป็น lexical** — ภาษาไทยที่ไม่มีเว้นวรรคจะ match ได้เฉพาะคำที่ตรง
7. **Gateway binary เป็น debug build** (`target/debug`) — ควรเปลี่ยนเป็น release ก่อนใช้งานจริง
8. **`PII_ENCRYPTION_KEY`** ถูกใส่ใน `/etc/talentos-gateway.env` แล้ว (ไม่กี่ชั่วโมงก่อน) — ข้อมูล PII ที่เข้ารหัสด้วยคีย์เก่าจะถอดไม่ได้ (tenant ถูกล้างไปแล้ว)
9. **EN toggle ยังแปลไม่ครบ** — nav และบางหน้า (payroll/settings/AI) เป็น EN แต่หน้าอื่นยังเป็นไทย; ควรแปลให้ครบหรือทำ label “EN (beta)” ก่อนใช้งานจริง

---

## 9. เอกสารที่เกี่ยวข้อง

| ไฟล์ | เนื้อหา |
|---|---|
| `docs/design/phase-1..4-spec.md` | สเปกทุกเฟส |
| `docs/superpowers/specs/2026-11-02-dashboard-sidebar-redesign-design.md` | ดีไซน์ sidebar + dashboard ตามบทบาท |
| `docs/superpowers/plans/2026-11-02-sidebar-shell-role-dashboard.md` | แผนงาน (ทำเสร็จแล้ว) |
| `docs/superpowers/specs/2026-11-02-ai-settings-knowledge-graph-design.md` | AI settings + KG + OpenBao |
| `docs/superpowers/plans/2026-11-02-ai-settings-knowledge-graph.md` | แผนงาน (ทำเสร็จแล้ว) |
| `docs/superpowers/specs/2026-11-03-ai-provider-model-roles-design.md` | multi-provider + model roles |
| `docs/superpowers/specs/2026-11-03-ai-providers-mcp-skills-agents-study.md` | ศึกษา provider/MCP/skills/agents + ดีไซน์ที่ยังไม่ลงโค้ด |
| `docs/BUILD_STATUS.md` | สถานะ build/feature แบบละเอียด |

---

## 10. คำถามที่ยังต้องตัดสิน (ก่อนทำ P0)

1. **stdio MCP** — เปิดใน v1 (พร้อม command allowlist) หรือรับเฉพาะ HTTP transport?
2. **Built-in skills** — seed ชุดเริ่มต้น 3 ตัว (leave policy / payroll anomaly / onboarding) หรือเริ่มว่าง?
3. **Agent เริ่มต้น** — seed "HR Assistant" ให้ทุกองค์กร หรือบังคับเลือก agent ทุกครั้ง?
