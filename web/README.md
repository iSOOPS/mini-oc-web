# SPA Build

```bash
cd web
npm install
npm run build       # outputs to dist/ which Dockerfile copies to /app/web
npm run dev         # dev server with proxy to BFF on :8100
```

For Docker builds, `npm run build` must be run locally (or via CI) so `dist/` is populated; the Dockerfile does not run npm.
