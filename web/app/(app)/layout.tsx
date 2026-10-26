import { cookies } from "next/headers";
import { redirect } from "next/navigation";
import Shell from "@/components/Shell";

export default function AppLayout({ children }: { children: React.ReactNode }) {
  const jar = cookies();
  // The access token may already be expired; the refresh token is enough to enter.
  if (!jar.has("talentos_at") && !jar.has("talentos_rt")) redirect("/login");
  return <Shell>{children}</Shell>;
}
