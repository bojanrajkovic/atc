## ATC Grafana dashboard

The canonical dashboard JSON lives **inside the Helm chart** at:

```
deploy/helm/atc/dashboards/atc-overview.json
```

### Why it moved

The dashboard ships as part of the chart so `helm package` finds it without symlink trickery — `helm package`'s file-walker does not consistently follow symlinks across versions. The previous standalone copy at `deploy/grafana/atc-postgres-overview.json` is gone; this directory now holds nothing but this redirect.

### Standalone import (no Kubernetes)

Operators who want to import the dashboard without the chart can:

1. Download the JSON from this repo:
   ```
   curl -L -o atc-overview.json \
     https://raw.githubusercontent.com/bojanrajkovic/atc/main/deploy/helm/atc/dashboards/atc-overview.json
   ```
2. In Grafana, **Dashboards → New → Import**, paste the JSON.
3. When prompted, pick your Prometheus datasource — the dashboard uses a `${datasource}` template variable so the picker works regardless of the datasource's name.

### Chart-bundled discovery (recommended)

See `docs/architecture/deployment.md` § "Grafana dashboard" for the opt-in toggles (`grafanaDashboard.enabled`) and the dual sidecar / grafana-operator discovery paths.
