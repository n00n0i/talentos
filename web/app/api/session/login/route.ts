import { NextRequest, NextResponse } from "next/server";

const API = process.env.TALENTOS_API_URL ?? "http://127.0.0.1:8080";

export async function POST(req: NextRequest) {
  const body = await req.text();
  let upstream: Response;
  try {
    upstream = await fetch(`${API}/api/v1/auth/login`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body,
    });
  } catch {
    return NextResponse.json(
      { error: "gateway_unreachable", message: "เชื่อมต่อเซิร์ฟเวอร์ไม่ได้" },
      { status: 502 },
    );
  }

  const payload = await upstream.json().catch(() => ({}));
  if (!upstream.ok) {
    return NextResponse.json(payload, { status: upstream.status });
  }

  const res = NextResponse.json({ ok: true });
  // On-prem installs often run plain HTTP behind a private network. Default to
  // Secure in production; TALENTOS_COOKIE_SECURE=false is the explicit opt-out.
  const secure =
    process.env.TALENTOS_COOKIE_SECURE !== undefined
      ? process.env.TALENTOS_COOKIE_SECURE === "true"
      : process.env.NODE_ENV === "production";
  res.cookies.set("talentos_at", payload.access_token, {
    httpOnly: true,
    sameSite: "lax",
    secure,
    path: "/",
    maxAge: payload.expires_in ?? 900,
  });
  res.cookies.set("talentos_rt", payload.refresh_token, {
    httpOnly: true,
    sameSite: "lax",
    secure,
    path: "/",
    maxAge: 60 * 60 * 24 * 30,
  });
  return res;
}
