//! Payslip PDF pipeline.
//!
//! HTML is rendered by headless Chromium rather than a Rust PDF library because
//! Thai text needs real shaping (HarfBuzz). A naive PDF writer would print broken
//! vowels — worse than refusing to produce a file.
//!
//! PDFs are cached on local disk: this platform is on-prem first, so local
//! storage is the correct default and keeps payslips available offline.

use std::path::{Path, PathBuf};

use uuid::Uuid;

use crate::RunError;

/// Where the renderer lives and where PDFs are cached.
#[derive(Debug, Clone)]
pub struct PdfConfig {
    pub chrome_bin: String,
    pub store_dir: PathBuf,
}

impl Default for PdfConfig {
    fn default() -> Self {
        Self {
            chrome_bin: std::env::var("CHROME_BIN")
                .unwrap_or_else(|_| "chromium-browser".to_string()),
            store_dir: std::env::var("PAYSLIP_STORE_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("./data/payslips")),
        }
    }
}

impl PdfConfig {
    fn pdf_path(&self, tenant_id: Uuid, payslip_id: Uuid) -> PathBuf {
        self.store_dir
            .join(tenant_id.to_string())
            .join(format!("{payslip_id}.pdf"))
    }
}

/// Cached PDF for this payslip, rendering it first when needed.
pub async fn get_or_render_pdf(
    cfg: &PdfConfig,
    tenant_id: Uuid,
    payslip_id: Uuid,
    html: &str,
) -> Result<Vec<u8>, RunError> {
    let target = cfg.pdf_path(tenant_id, payslip_id);
    if let Ok(bytes) = tokio::fs::read(&target).await {
        if bytes.starts_with(b"%PDF-") && bytes.len() > 1024 {
            return Ok(bytes);
        }
        // Truncated/corrupt cache: drop it and re-render.
        let _ = tokio::fs::remove_file(&target).await;
    }

    let bytes = render_pdf(cfg, html).await?;

    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| RunError::PdfFailed(format!("cannot create {}: {e}", parent.display())))?;
    }
    // Write via temp file + rename so a crash never leaves a partial PDF cached.
    let tmp = target.with_extension("pdf.tmp");
    tokio::fs::write(&tmp, &bytes)
        .await
        .map_err(|e| RunError::PdfFailed(format!("cannot write {}: {e}", tmp.display())))?;
    tokio::fs::rename(&tmp, &target)
        .await
        .map_err(|e| RunError::PdfFailed(format!("cannot store {}: {e}", target.display())))?;

    Ok(bytes)
}

/// HTML → PDF via headless Chromium.
pub async fn render_pdf(cfg: &PdfConfig, html: &str) -> Result<Vec<u8>, RunError> {
    let work_dir = cfg.store_dir.join("tmp");
    tokio::fs::create_dir_all(&work_dir)
        .await
        .map_err(|e| RunError::PdfFailed(format!("cannot create work dir: {e}")))?;

    let stem = Uuid::new_v4();
    let html_path = work_dir.join(format!("{stem}.html"));
    let pdf_path = work_dir.join(format!("{stem}.pdf"));
    tokio::fs::write(&html_path, html)
        .await
        .map_err(|e| RunError::PdfFailed(format!("cannot stage html: {e}")))?;

    let output = tokio::process::Command::new(&cfg.chrome_bin)
        .args([
            "--headless=new",
            "--disable-gpu",
            "--no-sandbox",
            "--hide-scrollbars",
            "--no-pdf-header-footer",
            &format!("--print-to-pdf={}", pdf_path.display()),
            &format!("file://{}", absolute(&html_path).display()),
        ])
        .output()
        .await
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => RunError::PdfUnavailable(format!(
                "`{}` not found — install Chromium or set CHROME_BIN",
                cfg.chrome_bin
            )),
            _ => RunError::PdfFailed(format!("failed to run renderer: {e}")),
        })?;

    let cleanup = |html: PathBuf, pdf: PathBuf| async move {
        let _ = tokio::fs::remove_file(html).await;
        let _ = tokio::fs::remove_file(pdf).await;
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        cleanup(html_path.clone(), pdf_path.clone()).await;
        return Err(RunError::PdfFailed(format!(
            "renderer exited with {}: {}",
            output.status,
            stderr.lines().take(3).collect::<Vec<_>>().join(" | ")
        )));
    }

    let bytes = match tokio::fs::read(&pdf_path).await {
        Ok(b) => b,
        Err(e) => {
            cleanup(html_path, pdf_path).await;
            return Err(RunError::PdfFailed(format!(
                "renderer produced no file: {e}"
            )));
        }
    };
    cleanup(html_path, pdf_path).await;

    if !bytes.starts_with(b"%PDF-") || bytes.len() <= 1024 {
        return Err(RunError::PdfFailed(
            "renderer output is not a valid PDF".to_string(),
        ));
    }
    Ok(bytes)
}

fn absolute(p: &Path) -> PathBuf {
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(p))
            .unwrap_or_else(|_| p.to_path_buf())
    }
}
