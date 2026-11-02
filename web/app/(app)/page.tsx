"use client";

import Link from "next/link";
import { useEffect, useState } from "react";
import Icon from "@/components/Icon";
import { gql, gw, handleApiError, Me, thDate } from "@/lib/api";
import {
  compactThb,
  otTrendQuery,
  primaryRole,
  relativeTh,
  sectionsFor,
  type SectionId,
} from "@/lib/dashboard";
import { useI18N } from "@/lib/i18n";

type Headcount = {
  active: number;
  probation: number;
  leave: number;
  by_department: { department: string | null; count: number }[];
};

type LatestRun = {
  periodStart: string;
  periodEnd: string;
  status: string;
  totalGross: string;
  totalNet: string;
  totalTax: string;
  totalSso: string;
  totalPf: string;
};

type SelfPayslip = { net_pay: string; period_start: string };
type SelfReview = { id: string };
type SelfBalance = { remaining_days: string };
type SelfOt = { id: string };

type LeaveApproval = {
  id: string;
  employee_name: string;
  employee_code: string;
  label_th: string;
  start_date: string;
  end_date: string;
};
type OtApproval = {
  id: string;
  employee_name: string;
  employee_code: string;
  work_date: string;
};
type ClaimApproval = {
  id: string;
  employee_name: string;
  employee_code: string;
  expense_date: string;
};
type Approvals = { leave: LeaveApproval[]; ot: OtApproval[]; claims: ClaimApproval[] };

const RUNS_QUERY =
  "{ payrollRuns(limit: 1) { periodStart periodEnd status totalGross totalNet totalTax totalSso totalPf } }";

const DRAFT_RUNS_QUERY = "{ payrollRuns(limit: 24) { status } }";

const KPI_IDS: SectionId[] = ["headcount", "net", "tax", "approvals", "ownPayslip"];

function Kpi({
  id,
  label,
  value,
  hint,
}: {
  id: string;
  label: string;
  value: string;
  hint?: string;
}) {
  return (
    <div className="card kpi" data-testid={`kpi-${id}`}>
      <div className="kpi-label">{label}</div>
      <div className="kpi-value">{value}</div>
      {hint ? <div className="kpi-hint">{hint}</div> : null}
    </div>
  );
}

/** One fetch for the KPI and the card grid; any failed queue fails the whole card. */
function useApprovals(enabled: boolean) {
  const [approvals, setApprovals] = useState<Approvals | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    if (!enabled) return;
    let alive = true;
    (async () => {
      try {
        const [leave, ot, claims] = await Promise.all([
          gw<{ requests: LeaveApproval[] }>("api/v1/leave/approvals"),
          gw<{ requests: OtApproval[] }>("api/v1/ot/approvals"),
          gw<{ claims: ClaimApproval[] }>("api/v1/claims/approvals"),
        ]);
        if (!alive) return;
        setApprovals({
          leave: leave.requests ?? [],
          ot: ot.requests ?? [],
          claims: claims.claims ?? [],
        });
      } catch {
        if (alive) setFailed(true);
      }
    })();
    return () => {
      alive = false;
    };
  }, [enabled]);

  return { approvals, failed };
}

function ApprovalsKpi({
  approvals,
  failed,
}: {
  approvals: Approvals | null;
  failed: boolean;
}) {
  const { t } = useI18N();
  const counts = approvals
    ? {
        leave: approvals.leave.length,
        ot: approvals.ot.length,
        claims: approvals.claims.length,
      }
    : null;

  const total = counts ? counts.leave + counts.ot + counts.claims : 0;
  const hint = failed
    ? t("dash.cardError")
    : counts && total === 0
      ? t("dash.empty.approvals")
      : counts
        ? `${t("nav.leave")} ${counts.leave} · ${t("nav.ot")} ${counts.ot} · ${t("nav.claims")} ${counts.claims}`
        : undefined;

  return (
    <Kpi
      id="approvals"
      label={t("dash.kpi.approvals")}
      value={counts ? String(total) : "—"}
      hint={hint}
    />
  );
}

type ApprovalCard = {
  key: string;
  kind: "leave" | "ot" | "claim";
  employee: string;
  code: string;
  when: string;
  date: string;
  href: string;
};

const APPROVAL_KIND: Record<ApprovalCard["kind"], string> = {
  leave: "dash.approvals.leave",
  ot: "dash.approvals.ot",
  claim: "dash.approvals.claim",
};

function approvalCards(approvals: Approvals): ApprovalCard[] {
  const range = (start: string, end: string) =>
    start === end ? thDate(start) : `${thDate(start)} – ${thDate(end)}`;
  return [
    ...approvals.leave.map((r) => ({
      key: `leave-${r.id}`,
      kind: "leave" as const,
      employee: r.employee_name,
      code: r.employee_code,
      when: r.start_date,
      date: `${r.label_th} · ${range(r.start_date, r.end_date)}`,
      href: "/leave",
    })),
    ...approvals.ot.map((r) => ({
      key: `ot-${r.id}`,
      kind: "ot" as const,
      employee: r.employee_name,
      code: r.employee_code,
      when: r.work_date,
      date: thDate(r.work_date),
      href: "/ot",
    })),
    ...approvals.claims.map((r) => ({
      key: `claim-${r.id}`,
      kind: "claim" as const,
      employee: r.employee_name,
      code: r.employee_code,
      when: r.expense_date,
      date: thDate(r.expense_date),
      href: "/claims",
    })),
  ]
    .sort((a, b) => b.when.localeCompare(a.when))
    .slice(0, 6);
}

function ApprovalsGrid({
  approvals,
  failed,
}: {
  approvals: Approvals | null;
  failed: boolean;
}) {
  const { t } = useI18N();
  const cards = approvals ? approvalCards(approvals) : [];

  return (
    <div className="card" data-testid="approvals-grid">
      <div className="card-head">
        <h2>{t("dash.approvals.title")}</h2>
      </div>
      <div className="appr-body">
        {approvals === null && !failed ? null : cards.length ? (
          <div className="appr-grid">
            {cards.map((card) => (
              <Link
                className="appr"
                href={card.href}
                key={card.key}
                data-testid="approval-card"
              >
                <span className="appr-head">
                  <span className="chip">{t(APPROVAL_KIND[card.kind])}</span>
                  <span className="appr-when">{card.date}</span>
                </span>
                <span className="appr-name">{card.employee}</span>
                <span className="appr-code">{card.code}</span>
              </Link>
            ))}
          </div>
        ) : (
          <p className="appr-empty">{t("dash.empty.approvals")}</p>
        )}
      </div>
    </div>
  );
}

type FollowUpData = { docs: number; drafts: number; cycles: number };

/** One fetch for all three counters; any failed call fails the whole card. */
function useFollowUps(enabled: boolean) {
  const [data, setData] = useState<FollowUpData | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    if (!enabled) return;
    let alive = true;
    Promise.all([
      gw<{ documents: unknown[] }>("api/v1/documents/expiring"),
      gql<{ payrollRuns: { status: string }[] }>(DRAFT_RUNS_QUERY),
      gw<{ cycles: { status: string }[] }>("api/v1/performance/cycles"),
    ])
      .then(([docs, runs, cycles]) => {
        if (!alive) return;
        setData({
          docs: docs.documents?.length ?? 0,
          drafts: runs.payrollRuns.filter((r) => r.status === "draft").length,
          cycles: cycles.cycles.filter((c) => c.status === "active").length,
        });
      })
      .catch(() => {
        if (alive) setFailed(true);
      });
    return () => {
      alive = false;
    };
  }, [enabled]);

  return { data, failed };
}

function FollowUpsCard({ data, failed }: { data: FollowUpData | null; failed: boolean }) {
  const { t } = useI18N();
  const rows = [
    { key: "docs", label: t("dash.followUps.docs"), href: "/documents" },
    { key: "drafts", label: t("dash.followUps.drafts"), href: "/payroll" },
    { key: "cycles", label: t("dash.followUps.cycles"), href: "/performance" },
  ] as const;
  const total = data ? data.docs + data.drafts + data.cycles : 0;

  return (
    <div className="card" data-testid="follow-ups">
      <div className="card-head">
        <h2>{t("dash.followUps.title")}</h2>
      </div>
      <div className="follow">
        {failed ? (
          <p className="follow-empty">{t("dash.cardError")}</p>
        ) : data === null ? null : total === 0 ? (
          <p className="follow-empty">{t("dash.empty.followUps")}</p>
        ) : (
          rows.map((row) => (
            <Link className="follow-row" href={row.href} key={row.key}>
              <span>{row.label}</span>
              <span className="follow-count">{data[row.key]}</span>
            </Link>
          ))
        )}
      </div>
    </div>
  );
}

type OtRow = { month: string; ot_minutes: number };

function useOtTrend(enabled: boolean) {
  const [rows, setRows] = useState<OtRow[] | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    if (!enabled) return;
    let alive = true;
    gw<{ rows: OtRow[] }>(`api/v1/reports/ot-trend?${otTrendQuery()}`)
      .then((data) => {
        if (alive) setRows(data.rows ?? []);
      })
      .catch(() => {
        if (alive) setFailed(true);
      });
    return () => {
      alive = false;
    };
  }, [enabled]);

  return { rows, failed };
}

const CHART_W = 480;
const CHART_H = 170;
const CHART_PAD = 10;
const CHART_LABEL_H = 22;

function monthLabel(month: string) {
  const [year, monthIndex] = month.split("-").map(Number);
  if (!year || !monthIndex) return month;
  return new Date(year, monthIndex - 1, 1).toLocaleDateString("th-TH", { month: "short" });
}

function OtChart({ rows, failed }: { rows: OtRow[] | null; failed: boolean }) {
  const { t } = useI18N();
  const max = rows?.length ? Math.max(...rows.map((row) => row.ot_minutes)) : 0;
  const hasOt = max > 0;
  const slot = rows?.length ? (CHART_W - CHART_PAD * 2) / rows.length : 0;
  const barWidth = Math.min(28, slot * 0.6);

  return (
    <div className="card" data-testid="ot-chart">
      <div className="card-head">
        <h2>{t("dash.ot.title")}</h2>
      </div>
      <div className="chart">
        {failed ? (
          <p className="chart-empty">{t("dash.cardError")}</p>
        ) : rows === null ? null : !hasOt ? (
          <p className="chart-empty">{t("dash.empty.ot")}</p>
        ) : (
          <svg viewBox={`0 0 ${CHART_W} ${CHART_H}`} role="img" aria-label={t("dash.ot.title")}>
            {rows.map((row, index) => {
              const height = (row.ot_minutes / max) * (CHART_H - CHART_PAD - CHART_LABEL_H);
              const x = CHART_PAD + index * slot + (slot - barWidth) / 2;
              return (
                <g key={row.month}>
                  <rect
                    className="bar"
                    x={x}
                    y={CHART_H - CHART_LABEL_H - height}
                    width={barWidth}
                    height={height}
                    rx={3}
                  >
                    <title>{`${monthLabel(row.month)} · ${row.ot_minutes} ${t("dash.ot.minutes")}`}</title>
                  </rect>
                  <text
                    className="bar-label"
                    x={x + barWidth / 2}
                    y={CHART_H - 4}
                    textAnchor="middle"
                  >
                    {monthLabel(row.month)}
                  </text>
                </g>
              );
            })}
          </svg>
        )}
      </div>
    </div>
  );
}

function RunSummary({
  run,
  loaded,
  failed,
}: {
  run: LatestRun | null;
  loaded: boolean;
  failed: boolean;
}) {
  const { t } = useI18N();
  const rows = run
    ? [
        { key: "gross", label: t("dash.summary.gross"), value: compactThb(run.totalGross) },
        { key: "tax", label: t("dash.summary.tax"), value: compactThb(run.totalTax) },
        { key: "sso", label: t("dash.summary.sso"), value: compactThb(run.totalSso) },
        { key: "pf", label: t("dash.summary.pf"), value: compactThb(run.totalPf) },
        { key: "net", label: t("dash.summary.net"), value: compactThb(run.totalNet) },
      ]
    : [];

  return (
    <div className="card summary" data-testid="run-summary">
      <div className="summary-head">
        <h2>{t("dash.summary.title")}</h2>
        {run ? <span className="summary-period">{`${thDate(run.periodStart)} – ${thDate(run.periodEnd)}`}</span> : null}
      </div>
      {failed ? (
        <p className="summary-empty">{t("dash.cardError")}</p>
      ) : !run ? (
        loaded ? <p className="summary-empty">{t("dash.empty.summary")}</p> : null
      ) : (
        <>
          <div className="summary-rows">
            {rows.map((row) => (
              <div className={`summary-row${row.key === "net" ? " net" : ""}`} key={row.key}>
                <span>{row.label}</span>
                <b>{row.value}</b>
              </div>
            ))}
          </div>
          <Link className="summary-cta" href="/payroll">
            {t("dash.summary.cta")}
          </Link>
        </>
      )}
    </div>
  );
}

const DONUT_COLORS = [
  "var(--teal)",
  "var(--sage-strong)",
  "var(--sage)",
  "var(--warning)",
  "var(--text-3)",
];
const DONUT_RADIUS = 52;
const DONUT_CIRCUMFERENCE = 2 * Math.PI * DONUT_RADIUS;

type DeptSegment = { label: string; count: number; color: string };

/** Top 4 departments plus an "other" roll-up; empty names share the unknown bucket. */
function deptSegments(
  rows: Headcount["by_department"],
  unknown: string,
  other: string,
): DeptSegment[] {
  const buckets = new Map<string, number>();
  for (const row of rows) {
    const label = row.department?.trim() ? row.department : unknown;
    buckets.set(label, (buckets.get(label) ?? 0) + row.count);
  }
  const sorted = [...buckets.entries()].sort((a, b) => b[1] - a[1]);
  const segments = sorted
    .slice(0, 4)
    .map(([label, count]) => ({ label, count }));
  const rest = sorted.slice(4).reduce((sum, [, count]) => sum + count, 0);
  if (rest > 0) segments.push({ label: other, count: rest });
  return segments.map((segment, index) => ({
    ...segment,
    color: DONUT_COLORS[Math.min(index, DONUT_COLORS.length - 1)],
  }));
}

function DeptDonut({ headcount, failed }: { headcount: Headcount | null; failed: boolean }) {
  const { t } = useI18N();
  const total = headcount ? headcount.active + headcount.probation + headcount.leave : 0;
  const segments = headcount
    ? deptSegments(headcount.by_department, t("dash.unknownDept"), t("dash.dept.other"))
    : [];
  const bucketTotal = segments.reduce((sum, segment) => sum + segment.count, 0);
  let offset = 0;

  return (
    <div className="card donut" data-testid="dept-donut">
      <div className="card-head">
        <h2>{t("dash.dept.title")}</h2>
      </div>
      {failed ? (
        <p className="donut-empty">{t("dash.cardError")}</p>
      ) : headcount === null ? null : total === 0 || bucketTotal === 0 ? (
        <p className="donut-empty">{t("dash.empty.dept")}</p>
      ) : (
        <div className="donut-body">
          <svg viewBox="0 0 160 160" role="img" aria-label={t("dash.dept.title")}>
            <circle
              cx="80"
              cy="80"
              r={DONUT_RADIUS}
              fill="none"
              stroke="var(--line)"
              strokeWidth="16"
            />
            {segments.map((segment, index) => {
              const length = (segment.count / bucketTotal) * DONUT_CIRCUMFERENCE;
              const dash = `${length} ${DONUT_CIRCUMFERENCE - length}`;
              const dashOffset = -offset;
              offset += length;
              return (
                <circle
                  key={`${segment.label}-${index}`}
                  cx="80"
                  cy="80"
                  r={DONUT_RADIUS}
                  fill="none"
                  stroke={segment.color}
                  strokeWidth="16"
                  strokeDasharray={dash}
                  strokeDashoffset={dashOffset}
                  transform="rotate(-90 80 80)"
                >
                  <title>{`${segment.label}: ${segment.count}`}</title>
                </circle>
              );
            })}
            <text className="donut-total" x="80" y="78" textAnchor="middle">
              {total}
            </text>
            <text className="donut-total-label" x="80" y="94" textAnchor="middle">
              {t("dash.kpi.headcount")}
            </text>
          </svg>
          <ul className="donut-legend">
            {segments.map((segment, index) => (
              <li className="donut-legend-row" key={`${segment.label}-${index}`}>
                <span className="donut-dot" style={{ background: segment.color }} />
                <span className="donut-legend-label">{segment.label}</span>
                <span className="donut-legend-count">{segment.count}</span>
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}

type ActivityEntry = { action: string; created_at: string };

const ACTIVITY_LABELS: Record<string, string> = {
  "employee.create": "dash.activity.employeeCreate",
  "payroll.run.approve": "dash.activity.payrollApprove",
  "leave.decide": "dash.activity.leaveDecide",
  "attendance.import": "dash.activity.attendanceImport",
};

function useActivity(enabled: boolean) {
  const [entries, setEntries] = useState<ActivityEntry[] | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    if (!enabled) return;
    let alive = true;
    gw<{ entries: ActivityEntry[] }>("api/v1/audit?limit=4")
      .then((data) => {
        if (alive) setEntries(data.entries ?? []);
      })
      .catch(() => {
        if (alive) setFailed(true);
      });
    return () => {
      alive = false;
    };
  }, [enabled]);

  return { entries, failed };
}

function ActivityCard({
  entries,
  failed,
}: {
  entries: ActivityEntry[] | null;
  failed: boolean;
}) {
  const { t } = useI18N();

  return (
    <div className="card" data-testid="activity">
      <div className="card-head">
        <h2>{t("dash.activity.title")}</h2>
      </div>
      {failed ? (
        <p className="recent-empty">{t("dash.cardError")}</p>
      ) : entries === null ? null : entries.length === 0 ? (
        <p className="recent-empty">{t("dash.empty.activity")}</p>
      ) : (
        <ul className="recent">
          {entries.map((entry, index) => (
            <li className="recent-item" key={`${entry.action}-${entry.created_at}-${index}`}>
              <span className="recent-label">
                {ACTIVITY_LABELS[entry.action]
                  ? t(ACTIVITY_LABELS[entry.action])
                  : entry.action}
              </span>
              <span className="recent-when">{relativeTh(entry.created_at)}</span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

/** One fetch shared by the own-payslip KPI and the self-service tile. */
function useSelfPayslip(enabled: boolean) {
  const [payslips, setPayslips] = useState<SelfPayslip[] | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    if (!enabled) return;
    let alive = true;
    gw<{ payslips: SelfPayslip[] }>("api/v1/me/payslips")
      .then((data) => {
        if (alive) setPayslips(data.payslips ?? []);
      })
      .catch(() => {
        if (alive) setFailed(true);
      });
    return () => {
      alive = false;
    };
  }, [enabled]);

  return { payslips, failed };
}

function OwnPayslipKpi({
  payslips,
  failed,
}: {
  payslips: SelfPayslip[] | null;
  failed: boolean;
}) {
  const { t } = useI18N();
  const payslip = payslips?.[0] ?? null;
  const loaded = payslips !== null;

  const hint = failed
    ? t("dash.cardError")
    : loaded && !payslip
      ? t("dash.empty.ownPayslip")
      : payslip
        ? thDate(payslip.period_start)
        : undefined;

  return (
    <Kpi
      id="ownPayslip"
      label={t("dash.kpi.ownPayslip")}
      value={payslip ? compactThb(payslip.net_pay) : "—"}
      hint={hint}
    />
  );
}

type MineKey = "payslip" | "review" | "leave" | "ot";

function SelfServiceCard({
  payslips,
  payslipsFailed,
}: {
  payslips: SelfPayslip[] | null;
  payslipsFailed: boolean;
}) {
  const { t } = useI18N();
  // undefined = still loading, null = endpoint answered with nothing
  const [values, setValues] = useState<Partial<Record<MineKey, string | null>>>({});
  const [failed, setFailed] = useState<Partial<Record<MineKey, boolean>>>({});

  useEffect(() => {
    let alive = true;
    const put = (key: MineKey, value: string | null) => {
      if (alive) setValues((current) => ({ ...current, [key]: value }));
    };
    const fail = (key: MineKey) => {
      if (alive) setFailed((current) => ({ ...current, [key]: true }));
    };

    gw<{ reviews: SelfReview[] }>("api/v1/me/reviews")
      .then((data) => put("review", data.reviews.length ? String(data.reviews.length) : null))
      .catch(() => fail("review"));
    gw<{ balances: SelfBalance[] }>("api/v1/me/leave")
      .then((data) =>
        put(
          "leave",
          data.balances.length
            ? String(
                Math.round(
                  data.balances.reduce((sum, row) => sum + Number(row.remaining_days), 0),
                ),
              )
            : null,
        ),
      )
      .catch(() => fail("leave"));
    gw<{ requests: SelfOt[] }>("api/v1/me/ot")
      .then((data) => put("ot", data.requests.length ? String(data.requests.length) : null))
      .catch(() => fail("ot"));

    return () => {
      alive = false;
    };
  }, []);

  const tiles: { key: MineKey; href: string; label: string; empty: string; icon: "file" | "clock" }[] = [
    { key: "payslip", href: "/me/payslips", label: t("nav.myPayslips"), empty: t("dash.empty.ownPayslip"), icon: "file" },
    { key: "review", href: "/me/reviews", label: t("nav.myReviews"), empty: t("dash.empty.selfService"), icon: "file" },
    { key: "leave", href: "/leave", label: t("dash.mine.leave"), empty: t("dash.empty.selfService"), icon: "clock" },
    { key: "ot", href: "/ot", label: t("nav.ot"), empty: t("dash.empty.selfService"), icon: "clock" },
  ];

  return (
    <div className="card">
      <div className="card-head">
        <h2>{t("dash.mine.title")}</h2>
      </div>
      <div className="mine-body">
        <div className="mine-grid" data-testid="mine-grid">
          {tiles.map((tile) => {
            const value =
              tile.key === "payslip"
                ? payslips === null
                  ? undefined
                  : payslips[0]
                    ? compactThb(payslips[0].net_pay)
                    : null
                : values[tile.key];
            const tileFailed = tile.key === "payslip" ? payslipsFailed : failed[tile.key];
            const hint = tileFailed
              ? t("dash.cardError")
              : value === null
                ? tile.empty
                : undefined;
            return (
              <Link className="mine-tile" href={tile.href} key={tile.key}>
                <span className="mine-label">
                  <Icon name={tile.icon} size={13} />
                  {tile.label}
                </span>
                <span className="mine-value">
                  {value == null
                    ? "—"
                    : tile.key === "leave"
                      ? `${value} ${t("dash.mine.days")}`
                      : value}
                </span>
                {hint ? <span className="mine-hint">{hint}</span> : null}
              </Link>
            );
          })}
        </div>
      </div>
    </div>
  );
}

export default function DashboardPage() {
  const { t } = useI18N();
  const [me, setMe] = useState<Me | null>(null);
  const [meFailed, setMeFailed] = useState(false);
  const [headcount, setHeadcount] = useState<Headcount | null>(null);
  const [headcountFailed, setHeadcountFailed] = useState(false);
  const [run, setRun] = useState<LatestRun | null>(null);
  const [runLoaded, setRunLoaded] = useState(false);
  const [runFailed, setRunFailed] = useState(false);

  const role = me ? primaryRole(me) : null;
  const sections = role ? sectionsFor(role) : [];
  const { approvals, failed: approvalsFailed } = useApprovals(
    sections.includes("approvals") || sections.includes("approvalsGrid"),
  );
  const { data: followUps, failed: followUpsFailed } = useFollowUps(
    sections.includes("followUps"),
  );
  const { rows: otRows, failed: otFailed } = useOtTrend(sections.includes("otTrend"));
  const { entries: activity, failed: activityFailed } = useActivity(
    sections.includes("activity"),
  );
  const { payslips: selfPayslips, failed: selfPayslipsFailed } = useSelfPayslip(
    sections.includes("ownPayslip") || sections.includes("selfService"),
  );

  useEffect(() => {
    gw<Me>("api/v1/auth/me")
      .then(setMe)
      .catch((e) => handleApiError(e, () => setMeFailed(true)));
  }, []);

  useEffect(() => {
    if (!role) return;
    const visible = sectionsFor(role);
    if (visible.includes("headcount")) {
      gw<Headcount>("api/v1/reports/headcount")
        .then(setHeadcount)
        .catch(() => setHeadcountFailed(true));
    }
    if (visible.includes("net") || visible.includes("tax")) {
      gql<{ payrollRuns: LatestRun[] }>(RUNS_QUERY)
        .then((data) => setRun(data.payrollRuns[0] ?? null))
        .catch(() => setRunFailed(true))
        .finally(() => setRunLoaded(true));
    }
  }, [role]);

  const headcountTotal = headcount
    ? headcount.active + headcount.probation + headcount.leave
    : 0;
  const runPeriod = run ? `${thDate(run.periodStart)} – ${thDate(run.periodEnd)}` : undefined;

  return (
    <>
      <div className="page-head">
        <div>
          <h1 data-testid="dash-title">{t("dash.title")}</h1>
          <p className="page-sub">{t("dash.subtitle")}</p>
        </div>
      </div>

      {meFailed && <p className="hint">{t("dash.cardError")}</p>}

      {sections.some((section) => KPI_IDS.includes(section)) && (
        <div className="kpi-grid" data-testid="kpi-row">
          {sections.includes("headcount") && (
            <Kpi
              id="headcount"
              label={t("dash.kpi.headcount")}
              value={headcount ? String(headcountTotal) : "—"}
              hint={
                headcountFailed
                  ? t("dash.cardError")
                  : headcount && headcountTotal === 0
                    ? t("dash.empty.headcount")
                    : undefined
              }
            />
          )}
          {sections.includes("net") && (
            <Kpi
              id="net"
              label={t("dash.kpi.net")}
              value={run ? compactThb(run.totalNet) : "—"}
              hint={
                runFailed
                  ? t("dash.cardError")
                  : run
                    ? runPeriod
                    : runLoaded
                      ? t("dash.empty.net")
                      : undefined
              }
            />
          )}
          {sections.includes("tax") && (
            <Kpi
              id="tax"
              label={t("dash.kpi.tax")}
              value={run ? compactThb(run.totalTax) : "—"}
              hint={
                runFailed
                  ? t("dash.cardError")
                  : run
                    ? runPeriod
                    : runLoaded
                      ? t("dash.empty.tax")
                      : undefined
              }
            />
          )}
          {sections.includes("approvals") && (
            <ApprovalsKpi approvals={approvals} failed={approvalsFailed} />
          )}
          {sections.includes("ownPayslip") && (
            <OwnPayslipKpi payslips={selfPayslips} failed={selfPayslipsFailed} />
          )}
        </div>
      )}

      <div className="dash-grid">
        <div className="stack">
          {sections.includes("approvalsGrid") && (
            <ApprovalsGrid approvals={approvals} failed={approvalsFailed} />
          )}
          {sections.includes("otTrend") && <OtChart rows={otRows} failed={otFailed} />}
          {sections.includes("activity") && (
            <ActivityCard entries={activity} failed={activityFailed} />
          )}
          {sections.includes("selfService") && (
            <SelfServiceCard payslips={selfPayslips} payslipsFailed={selfPayslipsFailed} />
          )}
        </div>
        <div className="stack">
          {sections.includes("followUps") && (
            <FollowUpsCard data={followUps} failed={followUpsFailed} />
          )}
          {sections.includes("runSummary") && (
            <RunSummary run={run} loaded={runLoaded} failed={runFailed} />
          )}
          {sections.includes("departments") && (
            <DeptDonut headcount={headcount} failed={headcountFailed} />
          )}
        </div>
      </div>
    </>
  );
}
