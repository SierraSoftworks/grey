# SSR Hydration Setup

This project now supports SSR hydration for real-time UI updates. Here's how to build and run it:

## Prerequisites

1. Install `wasm-pack`:
   ```bash
   curl https://rustwasm.github.io/wasm-pack/installer/init.sh -sSf | sh
   ```

## Building

### For Development

1. Build the WASM package:
   ```bash
   wasm-pack build --target web --out-dir pkg
   ```

2. Build and run the server:
   ```bash
   cargo run -- --config ./example/ui-testing.yml
   ```

### For Production

1. Build the WASM package:
   ```bash
   wasm-pack build --target web --out-dir pkg --release
   ```

2. Build and run the server:
   ```bash
   cargo build --release
   ./target/release/grey --config ./example/ui-testing.yml
   ```

## How it works

1. The server renders the initial HTML with SSR
2. The browser loads the page immediately with the server-rendered content
3. The WASM module loads and hydrates the existing DOM
4. Once hydrated, the client-side app takes over and:
   - Updates the status indicator every second
   - Fetches fresh data from `/api/v1/app-data` every 30 seconds
   - Provides real-time updates without page refresh

## API Endpoints

- `/` - Server-side rendered HTML page
- `/api/v1/app-data` - JSON data for client-side updates
- `/api/v1/probes` - List of all probes
- `/api/v1/probes/{probe}/history` - History for a specific probe
- `/static/*` - WASM files and static assets

## Features

- **Fast Initial Load**: Server-side rendering provides immediate content
- **Real-time Updates**: Client-side hydration enables live data updates
- **Status Indicator**: Shows how fresh the data is with color-coded status
- **Responsive Design**: Works on desktop and mobile
- **Progressive Enhancement**: Works even if JavaScript is disabled (initial load)
