import { NextResponse } from "next/server";

export async function POST() {
  const res = NextResponse.json({ ok: true });
  res.cookies.delete("talentos_at");
  res.cookies.delete("talentos_rt");
  return res;
}
