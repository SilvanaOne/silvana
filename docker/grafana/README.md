# Silvana RPC Monitoring Setup with Grafana & Prometheus

This directory contains everything you need to monitor your Silvana RPC service with Grafana and Prometheus.

## 🚀 Quick Start

### 1. Start your Silvana RPC service

```bash
# From the project root
cargo run
```

Your service will expose metrics on `http://localhost:9090/metrics`

### 2. Start the monitoring stack

```bash
# From the grafana directory
cd grafana
docker compose up -d
```

This starts:

- **Prometheus** on `http://localhost:9091` (scrapes metrics from your RPC service)
- **Grafana** on `http://localhost:3000` (visualizes the metrics)

### 3. Access Grafana

1. Open `http://localhost:3000` in your browser
2. Login with:
   - **Username**: `admin`
   - **Password**: `silvana123`
3. Navigate to **Dashboards > Silvana RPC Service Dashboard**

## 📊 Dashboard Overview

The dashboard includes these key panels:

### 🔴 Health Status Panels

- **Circuit Breaker Status**: Shows if the circuit breaker is OPEN (🚨) or CLOSED (✅)
- **Buffer Health Status**: Overall system health indicator

### 📈 Performance Metrics

- **Event Processing Rate**: Events received, processed, dropped, and failed per second
- **Current Buffer Size**: Number of events currently in buffer
- **Buffer Memory Usage**: Memory consumption in bytes
- **gRPC Request Rate**: HTTP requests per second
- **gRPC Request Duration**: P50, P95, P99 latency percentiles

### 🚨 Alerts

- Built-in alert when circuit breaker opens (indicates system overload)

## 🔧 Configuration Files

```
grafana/
├── docker compose.yml          # Docker services configuration
├── prometheus.yml              # Prometheus scraping configuration
├── provisioning/
│   ├── datasources/
│   │   └── prometheus.yml      # Auto-configure Prometheus datasource
│   └── dashboards/
│       └── silvana.yml         # Auto-load dashboard configuration
└── dashboards/
    └── silvana-rpc-dashboard.json  # The main dashboard
```

## 🔄 Managing the Stack

### Start monitoring

```bash
docker compose up -d
```

### Stop monitoring

```bash
docker compose down
```

### View logs

```bash
docker compose logs -f grafana
docker compose logs -f prometheus
```

### Update dashboard

Edit `dashboards/silvana-rpc-dashboard.json` and restart:

```bash
docker compose restart grafana
```

## 🎨 Customization

### Add new metrics

1. Add metrics to your Rust code using the `prometheus` crate
2. Update `src/monitoring.rs` to register new metrics
3. Add panels to the Grafana dashboard JSON
4. Restart Grafana: `docker compose restart grafana`

### Change refresh rate

Edit the dashboard JSON: `"refresh": "5s"` (default is 5 seconds)

### Add more datasources

Add configuration files to `provisioning/datasources/`

Happy monitoring! 🚀📊
