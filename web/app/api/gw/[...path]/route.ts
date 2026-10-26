import { NextRequest, NextResponse } from "next/server";

const API = process.env.TALENTOS_API_URL ?? "http://127.0.0.1:8080";

// Concurrent requests after an access token expires must not rotate the refresh
// token twice: the gateway revokes the whole token family on reuse. One promise
// per refresh token keeps the rotation single-flight.
const refreshing = new Map<string, Promise<Record<string, unknown> | null>>();

async function refresh(refreshToken: string) {
  const existing = refreshing.get(refreshToken);
  if (existing) return existing;

  const pending = (async () => {
    try {
      const res = await fetch(`${API}/api/v1/auth/refresh`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ refresh_token: refreshToken }),
      });
      if (!res.ok) return null;
      return (await res.json()) as Record<string, unknown>;
    } finally {
      refreshing.delete(refreshToken);
    }
  })();

  refreshing.set(refreshToken, pending);
  return pending;
}

function setTokens(res: NextResponse, tokens: Record<string, unknown>) {
  // On-prem installs often run plain HTTP behind a private network. Default to
  // Secure in production; TALENTOS_COOKIE_SECURE=false is the explicit opt-out.
  const secure =
    process.env.TALENTOS_COOKIE_SECURE !== undefined
      ? process.env.TALENTOS_COOKIE_SECURE === "true"
      : process.env.NODE_ENV === "production";
  res.cookies.set("talentos_at", String(tokens.access_token), {
    httpOnly: true,
    sameSite: "lax",
    secure,
    path: "/",
    maxAge: Number(tokens.expires_in ?? 900),
  });
  res.cookies.set("talentos_rt", String(tokens.refresh_token), {
    httpOnly: true,
    sameSite: "lax",
    secure,
    path: "/",
    maxAge: 60 * 60 * 24 * 30,
  });
}

async function forward(req: NextRequest, path: string[], accessToken: string | undefined) {
  const search = new URL(req.url).search;
  const target = `${API}/${path.join("/")}${search}`;

  const headers = new Headers();
  for (const name of ["content-type", "accept", "x-talentos-allow-destructive"]) {
    const value = req.headers.get(name);
    if (value) headers.set(name, value);
  }
  if (accessToken) headers.set("authorization", `Bearer ${accessToken}`);

  const hasBody = !["GET", "HEAD"].includes(req.method);
  return fetch(target, {
    method: req.method,
    headers,
    body: hasBody ? await req.arrayBuffer() : undefined,
    cache: "no-store",
    redirect: "manual",
  });
}

async function proxy(req: NextRequest, path: string[]) {
  const access = req.cookies.get("talentos_at")?.value;
  let upstream: Response;
  try {
    upstream = await forward(req, path, access);
  } catch {
    return NextResponse.json(
      { error: "gateway_unreachable", message: "เชื่อมต่อเซิร์ฟเวอร์ไม่ได้" },
      { status: 502 },
    );
  }

  let rotated: Record<string, unknown> | null = null;
  if (upstream.status === 401) {
    const refreshToken = req.cookies.get("talentos_rt")?.value;
    if (refreshToken) {
      rotated = await refresh(refreshToken);
      if (rotated) {
        try {
          upstream = await forward(req, path, String(rotated.access_token));
        } catch {
          return NextResponse.json(
            { error: "gateway_unreachable", message: "เชื่อมต่อเซิร์ฟเวอร์ไม่ได้" },
            { status: 502 },
          );
        }
      }
    }
  }

  const contentType = upstream.headers.get("content-type") ?? "application/json";
  const passthrough = contentType.startsWith("application/pdf")
    ? upstream.body
    : await upstream.arrayBuffer();

  const res = new NextResponse(passthrough, {
    status: upstream.status,
    headers: {
      "content-type": contentType,
      "cache-control": "no-store",
    },
  });

  if (contentType.startsWith("application/pdf")) {
    const disposition = upstream.headers.get("content-disposition");
    if (disposition) res.headers.set("content-disposition", disposition);
  }

  if (rotated) setTokens(res, rotated);
  if (upstream.status === 401 && !rotated) {
    res.cookies.delete("talentos_at");
    res.cookies.delete("talentos_rt");
  }
  return res;
}

type Ctx = { params: { path: string[] } };

export async function GET(req: NextRequest, ctx: Ctx) {
  return proxy(req, ctx.params.path);
}
export async function POST(req: NextRequest, ctx: Ctx) {
  return proxy(req, ctx.params.path);
}
export async function PUT(req: NextRequest, ctx: Ctx) {
  return proxy(req, ctx.params.path);
}
export async function DELETE(req: NextRequest, ctx: Ctx) {
  return proxy(req, ctx.params.path);
}
