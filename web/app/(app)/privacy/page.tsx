"use client";

import { useCallback, useEffect, useState } from "react";
import { ApiError, gw, handleApiError, thDate } from "@/lib/api";

type PrivacyStatus = { notice_version: string; accepted_at: string | null };

const bangkokToday = () =>
  new Intl.DateTimeFormat("en-CA", { timeZone: "Asia/Bangkok" }).format(new Date());

export default function PrivacyPage() {
  const [status, setStatus] = useState<PrivacyStatus | null>(null);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    setStatus(await gw<PrivacyStatus>("api/v1/me/privacy"));
  }, []);

  useEffect(() => {
    load().catch((e) => handleApiError(e, setError));
  }, [load]);

  async function accept() {
    if (!status) return;
    setBusy(true);
    setError("");
    try {
      const res = await gw<PrivacyStatus>("api/v1/me/privacy/consent", {
        method: "POST",
        body: JSON.stringify({ version: status.notice_version }),
      });
      setStatus(res);
      setNotice("บันทึกการรับทราบแล้ว");
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  async function exportData() {
    setBusy(true);
    setError("");
    setNotice("");
    try {
      const res = await fetch("/api/gw/api/v1/me/data-export", { cache: "no-store" });
      if (!res.ok) throw new ApiError(res.statusText || `HTTP ${res.status}`, res.status);
      const blob = await res.blob();
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = `talentos-personal-data-${bangkokToday()}.json`;
      a.click();
      URL.revokeObjectURL(url);
      setNotice("ดาวน์โหลดข้อมูลส่วนบุคคลแล้ว");
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <div className="page-head">
        <div>
          <h1>ความเป็นส่วนตัวและข้อมูลส่วนบุคคล</h1>
          <p className="page-sub">ประกาศความเป็นส่วนตัว สิทธิของเจ้าของข้อมูล และการส่งออกข้อมูล</p>
        </div>
      </div>

      {error && <p className="error">{error}</p>}
      {notice && <p className="success">{notice}</p>}

      <div className="card card-pad" style={{ display: "grid", gap: 14 }}>
        <section>
          <h2 style={{ fontSize: 16 }}>ข้อมูลที่เราเก็บ</h2>
          <p>
            ข้อมูลระบุตัวตนและข้อมูลติดต่อ ข้อมูลการจ้างงาน ค่าจ้าง ภาษี ประกันสังคม
            กองทุนสำรองเลี้ยงชีพ การลงเวลาและตำแหน่งขณะลงเวลา การลา OT ค่าใช้จ่าย
            เงินกู้ และเอกสารที่เกี่ยวข้องกับงานบุคคล
          </p>
        </section>
        <section>
          <h2 style={{ fontSize: 16 }}>วัตถุประสงค์</h2>
          <p>
            เพื่อบริหารงานบุคคล จ่ายค่าจ้าง คำนวณภาษีและเงินสมทบ จัดการสวัสดิการ
            ปฏิบัติตามกฎหมายแรงงานและภาษี และปกป้องสิทธิของทั้งพนักงานและองค์กร
          </p>
        </section>
        <section>
          <h2 style={{ fontSize: 16 }}>ระยะเวลาการเก็บรักษา</h2>
          <p>
            เก็บเท่าที่จำเป็นต่อการจ้างงาน และเก็บต่อตามที่กฎหมายกำหนด เช่น เอกสารภาษี
            และบัญชีค่าจ้างอย่างน้อย 5 ปี เมื่อพ้นกำหนด องค์กรจะลบหรือทำให้ anonymise
            ตามนโยบายที่ใช้บังคับ
          </p>
        </section>
        <section>
          <h2 style={{ fontSize: 16 }}>สิทธิของท่าน</h2>
          <p>
            ขอเข้าถึงและรับสำเนาข้อมูล ขอแก้ไขข้อมูล inaccurate ขอให้ลบหรือทำลายเมื่อพ้น
            ระยะเก็บรักษา คัดค้านหรือจำกัดการประมวลผล และร้องเรียนต่อหน่วยงานกำกับดูแล
            ติดต่อฝ่ายบุคคลขององค์กรเพื่อใช้สิทธิ
          </p>
        </section>
        <section>
          <h2 style={{ fontSize: 16 }}>ส่งออกข้อมูลของฉัน (DSAR)</h2>
          <p>
            ดาวน์โหลดข้อมูลส่วนบุคคลที่ระบบเก็บเกี่ยวกับบัญชีนี้ในรูปแบบ JSON
            รวมถึงข้อมูลพนักงาน การลงเวลา การลา OT ค่าใช้จ่าย เงินกู้ และสลิปค่าจ้าง
          </p>
          <button className="btn btn-primary" data-testid="export-data" disabled={busy} onClick={exportData}>
            ดาวน์โหลดข้อมูลของฉัน
          </button>
        </section>
      </div>

      <div className="card card-pad" style={{ marginTop: 12 }}>
        {status === null ? (
          <div className="spinner">กำลังโหลด…</div>
        ) : status.accepted_at ? (
          <p data-testid="consent-status">
            รับทราบประกาศความเป็นส่วนตัว version {status.notice_version} เมื่อ{" "}
            {thDate(status.accepted_at.slice(0, 10))}
          </p>
        ) : (
          <>
            <p>ท่านยังไม่ได้ยืนยันการรับทราบประกาศความเป็นส่วนตัว version {status.notice_version}</p>
            <button className="btn btn-primary" data-testid="accept-consent" disabled={busy} onClick={accept}>
              ยอมรับและรับทราบประกาศ
            </button>
          </>
        )}
      </div>
    </>
  );
}
