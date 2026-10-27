import type { MetadataRoute } from "next";

export default function manifest(): MetadataRoute.Manifest {
  return {
    name: "TalentOS ลงเวลา",
    short_name: "TalentOS",
    start_url: "/clock",
    display: "standalone",
    background_color: "#e7e9e7",
    theme_color: "#2f5f6d",
    lang: "th",
  };
}
