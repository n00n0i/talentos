# UI preview

Real screenshots of the running app (1440×1000 desktop, 390×844 mobile).

Open `index.html` in a browser, or open the PNGs directly.

Live demo (while the local services are running):

- URL: http://localhost:3100
- Tenant: `e2e`
- Email: `e2e@example.com`
- Password: `E2ePassword123!`

Regenerate:

```bash
# services: gateway :18108 with the e2e tenant + web :3100
docker compose exec -T -e PGPASSWORD=talentos_dev_secret postgres \
  psql -U talentos -d talentos -f - < scripts/seed-e2e.sql
cd web && PLAYWRIGHT_CHROMIUM_EXECUTABLE=/usr/bin/chromium-browser \
  E2E_BASE_URL=http://127.0.0.1:3100 npx playwright test
# then run the gallery script used to capture these pages
```
