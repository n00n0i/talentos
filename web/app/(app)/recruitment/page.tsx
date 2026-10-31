"use client";

import { useCallback, useEffect, useState } from "react";
import { gw, handleApiError, Me, thDate } from "@/lib/api";

type Job = {
  id: string;
  title: string;
  department: string | null;
  employment_type: string;
  headcount: number;
  status: "open" | "closed";
  description: string | null;
  candidate_count: number;
  hired_count: number;
  created_at: string;
};

type Candidate = {
  id: string;
  job_opening_id: string;
  first_name: string;
  last_name: string;
  email: string | null;
  phone: string | null;
  stage: string;
  source: string | null;
  note: string | null;
  has_cv: boolean;
  created_at: string;
};

type Interview = {
  id: string;
  candidate_id: string;
  scheduled_at: string;
  mode: string;
  location: string | null;
  status: string;
  notes: string | null;
};

type Offer = {
  id: string;
  candidate_id: string;
  candidate_name: string;
  position_title: string;
  salary: string;
  start_date: string;
  notes: string | null;
  status: string;
  created_at: string;
};

type StageEvent = {
  id: number;
  from_stage: string | null;
  to_stage: string;
  changed_by: string | null;
  changed_at: string;
};

const TYPES: Record<string, string> = {
  full_time: "พนักงานประจำ",
  part_time: "พาร์ทไทม์",
  contract: "สัญญาจ้าง",
  intern: "เด็กฝึกงาน",
};

const STAGES: Record<string, string> = {
  applied: "รับใบสมัคร",
  screening: "คัดกรอง",
  interview: "สัมภาษณ์",
  offer: "เสนองาน",
  hired: "จ้างแล้ว",
  rejected: "ไม่ผ่าน",
};

const emptyJob = {
  title: "",
  department: "",
  employment_type: "full_time",
  headcount: 1,
  description: "",
};

const emptyCandidate = {
  first_name: "",
  last_name: "",
  email: "",
  phone: "",
  source: "",
  note: "",
};

export default function RecruitmentPage() {
  const [jobs, setJobs] = useState<Job[]>([]);
  const [selected, setSelected] = useState<Job | null>(null);
  const [candidates, setCandidates] = useState<Candidate[]>([]);
  const [jobForm, setJobForm] = useState(emptyJob);
  const [candForm, setCandForm] = useState(emptyCandidate);
  const [cvFile, setCvFile] = useState<File | null>(null);
  const [historyFor, setHistoryFor] = useState<Candidate | null>(null);
  const [history, setHistory] = useState<StageEvent[]>([]);
  const [interviewFor, setInterviewFor] = useState<Candidate | null>(null);
  const [interviews, setInterviews] = useState<Interview[]>([]);
  const [offerFor, setOfferFor] = useState<Candidate | null>(null);
  const [offers, setOffers] = useState<Offer[]>([]);
  const [offerForm, setOfferForm] = useState({
    position_title: "",
    salary: "",
    start_date: "",
    notes: "",
  });
  const [interviewForm, setInterviewForm] = useState({
    scheduled_at: "",
    mode: "onsite",
    location: "",
    notes: "",
  });
  const [canWrite, setCanWrite] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");

  const loadJobs = useCallback(async () => {
    const res = await gw<{ jobs: Job[] }>("api/v1/jobs");
    setJobs(res.jobs);
    setSelected((current) => res.jobs.find((j) => j.id === current?.id) ?? null);
  }, []);

  const loadCandidates = useCallback(async (jobId: string) => {
    const res = await gw<{ candidates: Candidate[] }>(`api/v1/jobs/${jobId}/candidates`);
    setCandidates(res.candidates);
  }, []);

  useEffect(() => {
    loadJobs().catch((e) => handleApiError(e, setError));
    gw<Me>("api/v1/auth/me")
      .then((me) => setCanWrite(me.permissions.includes("employee.update")))
      .catch((e) => handleApiError(e, setError));
  }, [loadJobs]);

  useEffect(() => {
    if (!selected) {
      setCandidates([]);
      return;
    }
    loadCandidates(selected.id).catch((e) => handleApiError(e, setError));
  }, [selected, loadCandidates]);

  async function createJob(event: React.FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError("");
    setNotice("");
    try {
      const created = await gw<{ job: Job }>("api/v1/jobs", {
        method: "POST",
        body: JSON.stringify({
          ...jobForm,
          department: jobForm.department.trim() || null,
          description: jobForm.description.trim() || null,
        }),
      });
      setJobForm(emptyJob);
      await loadJobs();
      setSelected(created.job);
      setNotice("เปิดรับสมัครแล้ว");
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  async function toggleJob(job: Job) {
    setBusy(true);
    setError("");
    try {
      await gw(`api/v1/jobs/${job.id}`, {
        method: "PATCH",
        body: JSON.stringify({ status: job.status === "open" ? "closed" : "open" }),
      });
      await loadJobs();
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  async function removeJob(job: Job) {
    if (!confirm(`ลบตำแหน่ง ${job.title} และผู้สมัครทั้งหมดหรือไม่?`)) return;
    setBusy(true);
    setError("");
    try {
      await gw(`api/v1/jobs/${job.id}`, { method: "DELETE" });
      if (selected?.id === job.id) setSelected(null);
      await loadJobs();
      setNotice("ลบตำแหน่งแล้ว");
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  async function addCandidate(event: React.FormEvent) {
    event.preventDefault();
    if (!selected) return;
    setBusy(true);
    setError("");
    setNotice("");
    try {
      const created = await gw<{ candidate: Candidate }>(
        `api/v1/jobs/${selected.id}/candidates`,
        {
          method: "POST",
          body: JSON.stringify({
            first_name: candForm.first_name,
            last_name: candForm.last_name,
            email: candForm.email.trim() || null,
            phone: candForm.phone.trim() || null,
            source: candForm.source.trim() || null,
            note: candForm.note.trim() || null,
          }),
        },
      );
      setCandForm(emptyCandidate);
      const uploaded = cvFile ? await uploadCv(created.candidate.id, cvFile) : true;
      setCvFile(null);
      await Promise.all([loadCandidates(selected.id), loadJobs()]);
      setNotice(uploaded ? "เพิ่มผู้สมัครแล้ว" : "เพิ่มผู้สมัครแล้ว แต่ CV อัปโหลดไม่สำเร็จ");
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  async function uploadCv(id: string, file: File | undefined) {
    if (!file) return true;
    if (file.size > 5 * 1024 * 1024) {
      setError("ไฟล์ CV ใหญ่เกิน 5 MB");
      return false;
    }
    try {
      const res = await fetch(`/api/gw/api/v1/candidates/${id}/cv`, {
        method: "POST",
        headers: { "content-type": file.type },
        body: file,
      });
      if (!res.ok) {
        setError(`อัปโหลด CV ไม่สำเร็จ (${res.status})`);
        return false;
      }
      return true;
    } catch {
      setError("อัปโหลด CV ไม่สำเร็จ — ตรวจสอบการเชื่อมต่อ");
      return false;
    }
  }

  async function attachCv(id: string, file: File | undefined) {
    if (!file) return;
    setBusy(true);
    setError("");
    if (await uploadCv(id, file)) {
      await Promise.all([loadCandidates(selected!.id), loadJobs()]);
      setNotice("อัปโหลด CV แล้ว");
    }
    setBusy(false);
  }

  async function showHistory(candidate: Candidate) {
    setError("");
    try {
      const res = await gw<{ history: StageEvent[] }>(
        `api/v1/candidates/${candidate.id}/history`,
      );
      setHistory(res.history);
      setHistoryFor(candidate);
    } catch (e) {
      handleApiError(e, setError);
    }
  }

  async function showOffers(candidate: Candidate) {
    setError("");
    try {
      const res = await gw<{ offers: Offer[] }>(`api/v1/candidates/${candidate.id}/offers`);
      setOffers(res.offers);
      setOfferFor(candidate);
    } catch (e) {
      handleApiError(e, setError);
    }
  }

  async function createOffer(event: React.FormEvent) {
    event.preventDefault();
    if (!offerFor) return;
    setBusy(true);
    setError("");
    try {
      await gw(`api/v1/candidates/${offerFor.id}/offers`, {
        method: "POST",
        body: JSON.stringify({
          position_title: offerForm.position_title,
          salary: offerForm.salary,
          start_date: offerForm.start_date,
          notes: offerForm.notes.trim() || null,
        }),
      });
      setOfferForm({ position_title: "", salary: "", start_date: "", notes: "" });
      await showOffers(offerFor);
      setNotice("สร้างข้อเสนอแล้ว");
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  async function setOfferStatus(id: string, status: string) {
    setBusy(true);
    setError("");
    try {
      await gw(`api/v1/offers/${id}`, {
        method: "PATCH",
        body: JSON.stringify({ status }),
      });
      if (offerFor) await showOffers(offerFor);
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  async function removeOffer(id: string) {
    if (!confirm("ลบข้อเสนอนี้หรือไม่?")) return;
    setBusy(true);
    setError("");
    try {
      await gw(`api/v1/offers/${id}`, { method: "DELETE" });
      if (offerFor) await showOffers(offerFor);
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  async function showInterviews(candidate: Candidate) {
    setError("");
    try {
      const res = await gw<{ interviews: Interview[] }>(
        `api/v1/candidates/${candidate.id}/interviews`,
      );
      setInterviews(res.interviews);
      setInterviewFor(candidate);
    } catch (e) {
      handleApiError(e, setError);
    }
  }

  async function scheduleInterview(event: React.FormEvent) {
    event.preventDefault();
    if (!interviewFor) return;
    setBusy(true);
    setError("");
    try {
      await gw(`api/v1/candidates/${interviewFor.id}/interviews`, {
        method: "POST",
        body: JSON.stringify({
          scheduled_at: new Date(interviewForm.scheduled_at).toISOString(),
          mode: interviewForm.mode,
          location: interviewForm.location.trim() || null,
          notes: interviewForm.notes.trim() || null,
        }),
      });
      setInterviewForm({ scheduled_at: "", mode: "onsite", location: "", notes: "" });
      await showInterviews(interviewFor);
      setNotice("นัดสัมภาษณ์แล้ว");
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  async function setInterviewStatus(id: string, status: string) {
    setBusy(true);
    setError("");
    try {
      await gw(`api/v1/interviews/${id}`, {
        method: "PATCH",
        body: JSON.stringify({ status }),
      });
      if (interviewFor) await showInterviews(interviewFor);
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  async function removeInterview(id: string) {
    if (!confirm("ลบนัดสัมภาษณ์นี้หรือไม่?")) return;
    setBusy(true);
    setError("");
    try {
      await gw(`api/v1/interviews/${id}`, { method: "DELETE" });
      if (interviewFor) await showInterviews(interviewFor);
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  async function moveStage(candidate: Candidate, stage: string) {
    setBusy(true);
    setError("");
    try {
      await gw(`api/v1/candidates/${candidate.id}`, {
        method: "PATCH",
        body: JSON.stringify({ stage }),
      });
      await Promise.all([loadCandidates(candidate.job_opening_id), loadJobs()]);
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  async function removeCandidate(candidate: Candidate) {
    if (!confirm(`ลบผู้สมัคร ${candidate.first_name} ${candidate.last_name} หรือไม่?`)) return;
    setBusy(true);
    setError("");
    try {
      await gw(`api/v1/candidates/${candidate.id}`, { method: "DELETE" });
      await Promise.all([loadCandidates(candidate.job_opening_id), loadJobs()]);
      setNotice("ลบผู้สมัครแล้ว");
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
          <h1>สรรหาบุคลากร</h1>
          <p className="page-sub">ตำแหน่งที่เปิดรับและสายงานผู้สมัคร</p>
        </div>
      </div>

      {error && <p className="error">{error}</p>}
      {notice && <p className="success">{notice}</p>}

      {canWrite && (
        <div className="card card-pad" style={{ marginBottom: 12 }}>
          <h2 style={{ fontSize: 16, marginBottom: 10 }}>เปิดตำแหน่งใหม่</h2>
          <form onSubmit={createJob} style={{ display: "grid", gap: 10 }}>
            <div className="grid-2" style={{ gridTemplateColumns: "2fr 1fr", gap: 12 }}>
              <label>
                ชื่อตำแหน่ง
                <input
                  data-testid="job-title"
                  value={jobForm.title}
                  onChange={(e) => setJobForm({ ...jobForm, title: e.target.value })}
                  required
                />
              </label>
              <label>
                แผนก
                <input
                  data-testid="job-department"
                  value={jobForm.department}
                  onChange={(e) => setJobForm({ ...jobForm, department: e.target.value })}
                />
              </label>
            </div>
            <div className="grid-2" style={{ gridTemplateColumns: "1fr 1fr 2fr", gap: 12 }}>
              <label>
                ประเภท
                <select
                  data-testid="job-type"
                  value={jobForm.employment_type}
                  onChange={(e) => setJobForm({ ...jobForm, employment_type: e.target.value })}
                >
                  {Object.entries(TYPES).map(([value, label]) => (
                    <option key={value} value={value}>
                      {label}
                    </option>
                  ))}
                </select>
              </label>
              <label>
                จำนวนที่รับ
                <input
                  data-testid="job-headcount"
                  type="number"
                  min={1}
                  max={100}
                  value={jobForm.headcount}
                  onChange={(e) =>
                    setJobForm({ ...jobForm, headcount: Number(e.target.value) || 1 })
                  }
                />
              </label>
              <label>
                รายละเอียด
                <input
                  data-testid="job-description"
                  value={jobForm.description}
                  onChange={(e) => setJobForm({ ...jobForm, description: e.target.value })}
                />
              </label>
            </div>
            <button className="btn btn-primary" data-testid="create-job" type="submit" disabled={busy}>
              เปิดรับสมัคร
            </button>
          </form>
        </div>
      )}

      <div className="card" style={{ marginBottom: 12 }}>
        {jobs.length === 0 ? (
          <div className="empty">ยังไม่มีตำแหน่งที่เปิดรับ</div>
        ) : (
          <table className="table">
            <thead>
              <tr>
                <th>ตำแหน่ง</th>
                <th>แผนก</th>
                <th>ประเภท</th>
                <th>สถานะ</th>
                <th>ผู้สมัคร</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {jobs.map((job) => (
                <tr key={job.id} data-testid="job-row">
                  <td>
                    <button
                      className="btn btn-sm"
                      data-testid="select-job"
                      onClick={() => setSelected(job)}
                    >
                      {job.title}
                    </button>
                    {job.description && (
                      <div style={{ color: "var(--muted)", fontSize: 13 }}>{job.description}</div>
                    )}
                  </td>
                  <td>{job.department ?? "—"}</td>
                  <td>{TYPES[job.employment_type] ?? job.employment_type}</td>
                  <td>
                    <span className={`badge ${job.status === "open" ? "success" : ""}`}>
                      {job.status === "open" ? "เปิดรับ" : "ปิดรับ"}
                    </span>
                  </td>
                  <td>
                    {job.candidate_count} คน · จ้างแล้ว {job.hired_count}/{job.headcount}
                  </td>
                  <td>
                    {canWrite && (
                      <>
                        <button
                          className="btn btn-sm"
                          data-testid="toggle-job"
                          disabled={busy}
                          onClick={() => toggleJob(job)}
                          style={{ marginRight: 6 }}
                        >
                          {job.status === "open" ? "ปิดรับ" : "เปิดใหม่"}
                        </button>
                        <button
                          className="btn btn-sm"
                          data-testid="delete-job"
                          disabled={busy}
                          onClick={() => removeJob(job)}
                        >
                          ลบ
                        </button>
                      </>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>

      {selected && (
        <div className="card card-pad">
          <div className="card-head">
            <h2 style={{ fontSize: 16 }}>ผู้สมัคร: {selected.title}</h2>
          </div>

          {canWrite && (
            <form onSubmit={addCandidate} style={{ display: "grid", gap: 10, marginBottom: 12 }}>
              <div className="grid-2" style={{ gridTemplateColumns: "1fr 1fr 1fr", gap: 12 }}>
                <label>
                  ชื่อ
                  <input
                    data-testid="cand-first"
                    value={candForm.first_name}
                    onChange={(e) => setCandForm({ ...candForm, first_name: e.target.value })}
                    required
                  />
                </label>
                <label>
                  นามสกุล
                  <input
                    data-testid="cand-last"
                    value={candForm.last_name}
                    onChange={(e) => setCandForm({ ...candForm, last_name: e.target.value })}
                    required
                  />
                </label>
                <label>
                  อีเมล
                  <input
                    data-testid="cand-email"
                    type="email"
                    value={candForm.email}
                    onChange={(e) => setCandForm({ ...candForm, email: e.target.value })}
                  />
                </label>
              </div>
              <div className="grid-2" style={{ gridTemplateColumns: "1fr 1fr 2fr", gap: 12 }}>
                <label>
                  โทรศัพท์
                  <input
                    data-testid="cand-phone"
                    value={candForm.phone}
                    onChange={(e) => setCandForm({ ...candForm, phone: e.target.value })}
                  />
                </label>
                <label>
                  แหล่งที่มา
                  <input
                    data-testid="cand-source"
                    value={candForm.source}
                    onChange={(e) => setCandForm({ ...candForm, source: e.target.value })}
                  />
                </label>
                <label>
                  หมายเหตุ
                  <input
                    data-testid="cand-note"
                    value={candForm.note}
                    onChange={(e) => setCandForm({ ...candForm, note: e.target.value })}
                  />
                </label>
              </div>
              <label>
                CV (PDF, JPEG, PNG ≤ 5 MB)
                <input
                  data-testid="cand-cv"
                  type="file"
                  accept="application/pdf,image/jpeg,image/png"
                  onChange={(e) => setCvFile(e.target.files?.[0] ?? null)}
                />
              </label>
              <button className="btn" data-testid="add-candidate" type="submit" disabled={busy}>
                เพิ่มผู้สมัคร
              </button>
            </form>
          )}

          {candidates.length === 0 ? (
            <div className="empty">ยังไม่มีผู้สมัครสำหรับตำแหน่งนี้</div>
          ) : (
            <table className="table">
              <thead>
                <tr>
                  <th>ชื่อ</th>
                  <th>ติดต่อ</th>
                  <th>แหล่งที่มา</th>
                  <th>CV</th>
                  <th>ขั้นตอน</th>
                  <th>สมัครเมื่อ</th>
                  {canWrite && <th />}
                </tr>
              </thead>
              <tbody>
                {candidates.map((candidate) => (
                  <tr key={candidate.id} data-testid="candidate-row">
                    <td>
                      {candidate.first_name} {candidate.last_name}{" "}
                      <button
                        className="badge"
                        data-testid="candidate-history-button"
                        onClick={() => showHistory(candidate)}
                      >
                        ประวัติ
                      </button>{" "}
                      <button
                        className="badge"
                        data-testid="candidate-interview-button"
                        onClick={() => showInterviews(candidate)}
                      >
                        สัมภาษณ์
                      </button>{" "}
                      <button
                        className="badge"
                        data-testid="candidate-offer-button"
                        onClick={() => showOffers(candidate)}
                      >
                        ข้อเสนอ
                      </button>
                      {candidate.note && (
                        <div style={{ color: "var(--muted)", fontSize: 13 }}>{candidate.note}</div>
                      )}
                    </td>
                    <td>
                      {candidate.email ?? "—"}
                      {candidate.phone && <div style={{ fontSize: 13 }}>{candidate.phone}</div>}
                    </td>
                    <td>{candidate.source ?? "—"}</td>
                    <td>
                      {candidate.has_cv ? (
                        <a
                          className="badge accent"
                          data-testid="candidate-cv"
                          href={`/api/gw/api/v1/candidates/${candidate.id}/cv`}
                          target="_blank"
                          rel="noreferrer"
                          style={{ marginRight: 6 }}
                        >
                          ดาวน์โหลด
                        </a>
                      ) : (
                        "—"
                      )}
                      {canWrite && (
                        <label className="badge" style={{ cursor: "pointer" }}>
                          อัปโหลด
                          <input
                            data-testid="candidate-cv-upload"
                            type="file"
                            accept="application/pdf,image/jpeg,image/png"
                            disabled={busy}
                            style={{ display: "none" }}
                            onChange={(e) => attachCv(candidate.id, e.target.files?.[0])}
                          />
                        </label>
                      )}
                    </td>
                    <td>
                      <select
                        data-testid="candidate-stage"
                        value={candidate.stage}
                        disabled={busy || !canWrite}
                        onChange={(e) => moveStage(candidate, e.target.value)}
                      >
                        {Object.entries(STAGES).map(([value, label]) => (
                          <option key={value} value={value}>
                            {label}
                          </option>
                        ))}
                      </select>
                    </td>
                    <td>{thDate(candidate.created_at.slice(0, 10))}</td>
                    {canWrite && (
                      <td>
                        <button
                          className="btn btn-sm"
                          data-testid="delete-candidate"
                          disabled={busy}
                          onClick={() => removeCandidate(candidate)}
                        >
                          ลบ
                        </button>
                      </td>
                    )}
                  </tr>
                ))}
              </tbody>
            </table>
          )}

          {offerFor && (
            <div className="card card-pad" data-testid="candidate-offers" style={{ marginTop: 12 }}>
              <h3 style={{ fontSize: 15 }}>
                ข้อเสนองาน: {offerFor.first_name} {offerFor.last_name}
              </h3>
              {canWrite && (
                <form onSubmit={createOffer} style={{ display: "grid", gap: 10, marginBottom: 12 }}>
                  <div className="grid-2" style={{ gridTemplateColumns: "2fr 1fr 1fr", gap: 12 }}>
                    <label>
                      ตำแหน่ง
                      <input
                        data-testid="offer-position"
                        value={offerForm.position_title}
                        onChange={(e) =>
                          setOfferForm({ ...offerForm, position_title: e.target.value })
                        }
                        required
                      />
                    </label>
                    <label>
                      เงินเดือน (บาท)
                      <input
                        data-testid="offer-salary"
                        type="number"
                        min={1}
                        step="0.01"
                        value={offerForm.salary}
                        onChange={(e) => setOfferForm({ ...offerForm, salary: e.target.value })}
                        required
                      />
                    </label>
                    <label>
                      วันที่เริ่มงาน
                      <input
                        data-testid="offer-start"
                        type="date"
                        value={offerForm.start_date}
                        onChange={(e) => setOfferForm({ ...offerForm, start_date: e.target.value })}
                        required
                      />
                    </label>
                  </div>
                  <label>
                    หมายเหตุ
                    <input
                      data-testid="offer-notes"
                      value={offerForm.notes}
                      onChange={(e) => setOfferForm({ ...offerForm, notes: e.target.value })}
                    />
                  </label>
                  <button className="btn" data-testid="create-offer" type="submit" disabled={busy}>
                    สร้างข้อเสนอ
                  </button>
                </form>
              )}

              {offers.length === 0 ? (
                <div className="empty">ยังไม่มีข้อเสนอ</div>
              ) : (
                <ul className="list">
                  {offers.map((offer) => (
                    <li key={offer.id} data-testid="offer-row">
                      {offer.position_title} · {Number(offer.salary).toLocaleString("th-TH")} บาท ·
                      เริ่ม {offer.start_date}
                      <a
                        className="badge accent"
                        data-testid="offer-pdf"
                        href={`/api/gw/api/v1/offers/${offer.id}/pdf`}
                        target="_blank"
                        rel="noreferrer"
                        style={{ marginLeft: 8 }}
                      >
                        PDF
                      </a>
                      <select
                        data-testid="offer-status"
                        value={offer.status}
                        disabled={busy || !canWrite}
                        onChange={(e) => setOfferStatus(offer.id, e.target.value)}
                        style={{ marginLeft: 8 }}
                      >
                        <option value="draft">ร่าง</option>
                        <option value="sent">ส่งแล้ว</option>
                        <option value="accepted">ตอบรับ</option>
                        <option value="declined">ปฏิเสธ</option>
                      </select>
                      {canWrite && (
                        <button
                          className="btn btn-sm"
                          data-testid="delete-offer"
                          disabled={busy}
                          onClick={() => removeOffer(offer.id)}
                          style={{ marginLeft: 6 }}
                        >
                          ลบ
                        </button>
                      )}
                    </li>
                  ))}
                </ul>
              )}
            </div>
          )}

          {interviewFor && (
            <div className="card card-pad" data-testid="candidate-interviews" style={{ marginTop: 12 }}>
              <h3 style={{ fontSize: 15 }}>
                นัดสัมภาษณ์: {interviewFor.first_name} {interviewFor.last_name}
              </h3>
              {canWrite && (
                <form onSubmit={scheduleInterview} style={{ display: "grid", gap: 10, marginBottom: 12 }}>
                  <div className="grid-2" style={{ gridTemplateColumns: "1fr 1fr 2fr", gap: 12 }}>
                    <label>
                      วันเวลา
                      <input
                        data-testid="interview-when"
                        type="datetime-local"
                        value={interviewForm.scheduled_at}
                        onChange={(e) =>
                          setInterviewForm({ ...interviewForm, scheduled_at: e.target.value })
                        }
                        required
                      />
                    </label>
                    <label>
                      รูปแบบ
                      <select
                        data-testid="interview-mode"
                        value={interviewForm.mode}
                        onChange={(e) =>
                          setInterviewForm({ ...interviewForm, mode: e.target.value })
                        }
                      >
                        <option value="onsite">ที่สำนักงาน</option>
                        <option value="online">ออนไลน์</option>
                        <option value="phone">โทรศัพท์</option>
                      </select>
                    </label>
                    <label>
                      สถานที่ / ลิงก์
                      <input
                        data-testid="interview-location"
                        value={interviewForm.location}
                        onChange={(e) =>
                          setInterviewForm({ ...interviewForm, location: e.target.value })
                        }
                      />
                    </label>
                  </div>
                  <label>
                    หมายเหตุ
                    <input
                      data-testid="interview-notes"
                      value={interviewForm.notes}
                      onChange={(e) =>
                        setInterviewForm({ ...interviewForm, notes: e.target.value })
                      }
                    />
                  </label>
                  <button className="btn" data-testid="schedule-interview" type="submit" disabled={busy}>
                    นัดสัมภาษณ์
                  </button>
                </form>
              )}

              {interviews.length === 0 ? (
                <div className="empty">ยังไม่มีการนัดสัมภาษณ์</div>
              ) : (
                <ul className="list">
                  {interviews.map((interview) => (
                    <li key={interview.id} data-testid="interview-row">
                      {new Date(interview.scheduled_at).toLocaleString("th-TH")} ·{" "}
                      {interview.mode === "online"
                        ? "ออนไลน์"
                        : interview.mode === "phone"
                          ? "โทรศัพท์"
                          : "ที่สำนักงาน"}
                      {interview.location ? ` · ${interview.location}` : ""}
                      {interview.notes ? ` · ${interview.notes}` : ""}
                      <select
                        data-testid="interview-status"
                        value={interview.status}
                        disabled={busy || !canWrite}
                        onChange={(e) => setInterviewStatus(interview.id, e.target.value)}
                        style={{ marginLeft: 8 }}
                      >
                        <option value="scheduled">นัดไว้</option>
                        <option value="completed">เสร็จสิ้น</option>
                        <option value="cancelled">ยกเลิก</option>
                      </select>
                      {canWrite && (
                        <button
                          className="btn btn-sm"
                          data-testid="delete-interview"
                          disabled={busy}
                          onClick={() => removeInterview(interview.id)}
                          style={{ marginLeft: 6 }}
                        >
                          ลบ
                        </button>
                      )}
                    </li>
                  ))}
                </ul>
              )}
            </div>
          )}

          {historyFor && (
            <div className="card card-pad" data-testid="candidate-history" style={{ marginTop: 12 }}>
              <h3 style={{ fontSize: 15 }}>
                ประวัติขั้นตอน: {historyFor.first_name} {historyFor.last_name}
              </h3>
              <ul className="list">
                {history.map((event) => (
                  <li key={event.id}>
                    {event.from_stage ? STAGES[event.from_stage] ?? event.from_stage : "เริ่มต้น"} →{" "}
                    {STAGES[event.to_stage] ?? event.to_stage}
                    <span style={{ marginLeft: 8, color: "var(--muted)" }}>
                      {new Date(event.changed_at).toLocaleString("th-TH")}
                    </span>
                  </li>
                ))}
              </ul>
            </div>
          )}
        </div>
      )}
    </>
  );
}
