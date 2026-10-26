import React from "react";

export type IconName =
  | "overview"
  | "users"
  | "clock"
  | "wallet"
  | "file"
  | "user"
  | "settings"
  | "logout"
  | "chevron"
  | "sparkle"
  | "clover";

const paths: Record<IconName, React.ReactNode> = {
  overview: (
    <>
      <rect x="3.5" y="3.5" width="7" height="7" rx="2" />
      <rect x="13.5" y="3.5" width="7" height="7" rx="2" />
      <rect x="3.5" y="13.5" width="7" height="7" rx="2" />
      <rect x="13.5" y="13.5" width="7" height="7" rx="2" />
    </>
  ),
  users: (
    <>
      <circle cx="9" cy="8" r="3.2" />
      <path d="M3.5 19.5c0-3 2.5-5 5.5-5s5.5 2 5.5 5" />
      <path d="M16 5.6a3 3 0 0 1 0 5.8" />
      <path d="M17.5 14.6c2 .6 3.5 2.3 3.5 4.9" />
    </>
  ),
  clock: (
    <>
      <circle cx="12" cy="12" r="8.2" />
      <path d="M12 7.8V12l2.8 1.9" />
    </>
  ),
  wallet: (
    <>
      <rect x="3.2" y="6" width="17.6" height="13" rx="3" />
      <path d="M3.2 10h17.6" />
      <path d="M15.5 15.2h2.2" />
    </>
  ),
  file: (
    <>
      <path d="M6 3.5h7l5 5v12H6z" />
      <path d="M13 3.5v5h5" />
      <path d="M9 13h6M9 16.5h6" />
    </>
  ),
  user: (
    <>
      <circle cx="12" cy="8" r="3.4" />
      <path d="M5 20c0-3.6 3.1-6 7-6s7 2.4 7 6" />
    </>
  ),
  settings: (
    <>
      <path d="M4 7h7M15 7h5M4 17h5M13 17h7" />
      <circle cx="13" cy="7" r="2.2" />
      <circle cx="11" cy="17" r="2.2" />
    </>
  ),
  logout: (
    <>
      <path d="M14 4.5H6.5A2.5 2.5 0 0 0 4 7v10a2.5 2.5 0 0 0 2.5 2.5H14" />
      <path d="M10.5 12h9M16.5 8.5 20 12l-3.5 3.5" />
    </>
  ),
  chevron: <path d="m7 10 5 5 5-5" />,
  clover: (
    <>
      <circle cx="9" cy="9" r="3.2" fill="currentColor" stroke="none" />
      <circle cx="15" cy="9" r="3.2" fill="currentColor" stroke="none" />
      <circle cx="9" cy="15" r="3.2" fill="currentColor" stroke="none" />
      <circle cx="15" cy="15" r="3.2" fill="currentColor" stroke="none" />
    </>
  ),
  sparkle: (
    <>
      <path d="M12 3.5c.6 4.2 1.8 5.4 6 6-4.2.6-5.4 1.8-6 6-.6-4.2-1.8-5.4-6-6 4.2-.6 5.4-1.8 6-6Z" />
      <path d="M18.5 15.5c.25 1.7.75 2.2 2.5 2.5-1.75.3-2.25.8-2.5 2.5-.25-1.7-.75-2.2-2.5-2.5 1.75-.3 2.25-.8 2.5-2.5Z" />
    </>
  ),
};

export default function Icon({
  name,
  size = 18,
  className,
}: {
  name: IconName;
  size?: number;
  className?: string;
}) {
  return (
    <svg
      className={`icon${className ? ` ${className}` : ""}`}
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.6"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      {paths[name]}
    </svg>
  );
}
