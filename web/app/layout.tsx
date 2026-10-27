import type { Metadata } from "next";
import "./globals.css";

export const metadata: Metadata = {
  title: "TalentOS",
  description: "ระบบ HR และเงินเดือนสำหรับองค์กรไทย",
  manifest: "/manifest.webmanifest",
};

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="th">
      <body>{children}</body>
    </html>
  );
}
