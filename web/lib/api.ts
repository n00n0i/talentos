export class ApiError extends Error {
  status: number;
  constructor(message: string, status: number) {
    super(message);
    this.status = status;
  }
}

/** Every browser call goes through the Next proxy, which adds the bearer token. */
export async function gw<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`/api/gw/${path.replace(/^\//, "")}`, {
    ...init,
    headers: { "content-type": "application/json", ...(init?.headers ?? {}) },
    cache: "no-store",
  });
  if (!res.ok) {
    let message = res.statusText || `HTTP ${res.status}`;
    try {
      const body = await res.json();
      message = body.message || body.error || message;
    } catch {
      // non-JSON error body
    }
    throw new ApiError(message, res.status);
  }
  if (res.status === 204) return undefined as T;
  return res.json();
}

export async function gql<T>(query: string, variables?: Record<string, unknown>): Promise<T> {
  const res = await gw<{ data?: T; errors?: { message: string }[] }>("api/v1/graphql", {
    method: "POST",
    body: JSON.stringify({ query, variables }),
  });
  if (res.errors?.length) throw new ApiError(res.errors[0].message, 200);
  return res.data as T;
}

export const thb = (value: string | number) =>
  new Intl.NumberFormat("th-TH", {
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  }).format(Number(value));

export const thbShort = (value: string | number) =>
  new Intl.NumberFormat("th-TH", { maximumFractionDigits: 0 }).format(Number(value));

export const thDate = (iso: string) =>
  new Date(`${iso}T00:00:00`).toLocaleDateString("th-TH", {
    day: "numeric",
    month: "short",
    year: "numeric",
  });

export const RUN_STATUS: Record<string, { label: string; tone: string }> = {
  draft: { label: "ฉบับร่าง", tone: "" },
  calculating: { label: "กำลังคำนวณ", tone: "warning" },
  calculated: { label: "คำนวณแล้ว", tone: "accent" },
  pending_approval: { label: "รออนุมัติ", tone: "warning" },
  approved: { label: "อนุมัติแล้ว", tone: "success" },
  paid: { label: "จ่ายแล้ว", tone: "success" },
  failed: { label: "ล้มเหลว", tone: "danger" },
};

export type Me = {
  user_id: string;
  email: string;
  full_name_th: string;
  full_name_en: string | null;
  tenant_id: string;
  tenant_name: string | null;
  entity_id: string | null;
  roles: string[];
  permissions: string[];
  last_login_at: string | null;
};

export type PayrollRun = {
  id: string;
  periodStart: string;
  periodEnd: string;
  paymentDate: string;
  status: string;
  totalEmployees: number;
  totalGross: string;
  totalNet: string;
  totalTax: string;
  totalSso: string;
  totalPf: string;
};

export type Payslip = {
  payslip_id: string;
  employee_id: string;
  base_salary: string;
  gross_earnings: string;
  net_pay: string;
  tax_withheld: string;
  sso_employee: string;
  pf_employee: string;
  status: string;
  anomalies: unknown;
};

/** Shared UI error handling: an expired session goes back to the login page. */
export function handleApiError(e: unknown, setError: (message: string) => void) {
  if (e instanceof ApiError && e.status === 401) {
    window.location.href = "/login";
    return;
  }
  setError(e instanceof Error ? e.message : "เกิดข้อผิดพลาด");
}
