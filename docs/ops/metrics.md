# Metrics and alerting

The gateway exposes two Prometheus text-metric endpoints:

- `GET /metrics` — process metrics (request counts, duration, uptime)
- `GET /business-metrics` — process metrics plus business gauges

## Process metrics

```
talentos_http_requests_total{status="2xx"}
talentos_http_requests_total{status="4xx"}
talentos_http_requests_total{status="5xx"}
talentos_http_requests_total{status="other"}   # 1xx/3xx
talentos_http_request_duration_seconds_sum
talentos_http_request_duration_seconds_count
talentos_uptime_seconds
```

## Business gauges

`GET /business-metrics` appends these gauges after the process metrics:

```
talentos_notification_outbox{channel="email",status="pending"}
talentos_notification_outbox{channel="email",status="sent"}
talentos_notification_outbox{channel="email",status="failed"}
talentos_notification_outbox{channel="line",status="pending"}
talentos_notification_outbox{channel="line",status="sent"}
talentos_notification_outbox{channel="line",status="failed"}
talentos_payroll_runs_by_status{status="draft"}
talentos_payroll_runs_by_status{status="calculating"}
talentos_payroll_runs_by_status{status="pending_approval"}
talentos_payroll_runs_by_status{status="approved"}
talentos_payroll_runs_by_status{status="rejected"}
talentos_payroll_runs_by_status{status="disbursing"}
talentos_payroll_runs_by_status{status="completed"}
talentos_payroll_runs_by_status{status="failed"}
talentos_payroll_oldest_stuck_run_seconds
```

Both endpoints respect the same `METRICS_TOKEN` bearer check. When the token
is unset they are public; set it to require `Authorization: Bearer <token>`.

## Alert webhook

The gateway can POST alerts to a configured webhook when thresholds are
crossed. No external service is required — a single outbound POST.

| Variable | Default | Description |
|---|---|---|
| `METRICS_ALERT_WEBHOOK` | *(unset)* | Full URL to POST alerts to. Unset → no alerts. |
| `METRICS_ALERT_PENDING_THRESHOLD` | `100` | Total pending notifications (all channels) to alert on |
| `METRICS_ALERT_STUCK_RUN_HOURS` | `24` | Oldest stuck payroll run age in hours to alert on |

The sweep runs every 60 seconds, starting 10 seconds after the gateway boots.
Each alert is a JSON POST with this shape:

```json
{ "alert": "talentos_notification_backlog_high", "pending": 250, "threshold": 100 }
{ "alert": "talentos_stuck_payroll_run", "age_hours": 30, "threshold_hours": 24 }
```

## Prometheus scrape

```yaml
scrape_configs:
  - job_name: talentos
    metrics_path: /business-metrics   # includes process + business gauges
    static_configs:
      - targets: ["gateway:8080"]
    # authorization:
    #   credentials: <METRICS_TOKEN>
```

## Example alerts

```yaml
groups:
  - name: talentos
    rules:
      - alert: TalentOSHigh5xxRate
        expr: |
          sum(rate(talentos_http_requests_total{status="5xx"}[5m]))
            /
          sum(rate(talentos_http_requests_total[5m])) > 0.05
        for: 10m
        labels: { severity: page }
        annotations:
          summary: More than 5% of gateway responses are 5xx

      - alert: TalentOSSlowRequests
        expr: |
          rate(talentos_http_request_duration_seconds_sum[5m])
            /
          rate(talentos_http_request_duration_seconds_count[5m]) > 1
        for: 10m
        labels: { severity: warning }
        annotations:
          summary: Average gateway request latency is above 1s

      - alert: TalentOSDown
        expr: up{job="talentos"} == 0
        for: 2m
        labels: { severity: page }
```

The average is a coarse signal; per-route histograms are intentionally not
exposed yet (cardinality).

Business gauges are exposed at `GET /business-metrics` — scrape that path when
you need them, or the Prometheus alert examples above for 5xx / latency /
uptime.
