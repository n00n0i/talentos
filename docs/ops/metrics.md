# Metrics and alerting

The gateway exposes Prometheus text metrics at `GET /metrics`.

```
talentos_http_requests_total{status="2xx|4xx|5xx"}
talentos_http_request_duration_seconds_sum
talentos_http_request_duration_seconds_count
talentos_uptime_seconds
```

The endpoint is public by default so a local Prometheus can scrape it. Set
`METRICS_TOKEN` to require `Authorization: Bearer <token>`, and keep the gateway
on a private network either way.

## Prometheus scrape

```yaml
scrape_configs:
  - job_name: talentos
    metrics_path: /metrics
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
exposed yet (cardinality). Business gauges (notification backlog, payroll runs)
are also not exposed yet.
