import { NextRequest, NextResponse } from "next/server";

const API = process.env.TALENTOS_API_URL ?? "http://127.0.0.1:8080";

export async function POST(req: NextRequest) {
  const body = await req.text();
  try {
    const upstream = await fetch(`${API}/api/v1/auth/reset-password`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body,
    });
    const payload = await upstream.json().catch(() => ({}));
    return NextResponse.json(payload, { status: upstream.status });
  } catch {
    return NextResponse.json(
      { error: "gateway_unreachable", message: "เชื่อมต่อเซิร์ฟเวอร์ไม่ได้" },
      { status: 502 },
    );
  }
}
