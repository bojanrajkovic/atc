#!/usr/bin/env python3
"""Capture GitHub webhook payloads for test fixtures.

Usage:
  1. Run this script:       python3 scripts/capture-webhooks.py
  2. In another terminal:   gh webhook forward --events=workflow_run,workflow_job --url=http://localhost:9876/
  3. Trigger a CI run (push to main, open a PR, etc.)
  4. Captured payloads land in tmp/webhook-captures/

Press Ctrl+C to stop.
"""

import json
import os
import sys
from datetime import datetime, timezone
from http.server import HTTPServer, BaseHTTPRequestHandler

CAPTURE_DIR = os.path.join(os.path.dirname(__file__), "..", "tmp", "webhook-captures")


class WebhookHandler(BaseHTTPRequestHandler):
    def do_POST(self):
        content_length = int(self.headers.get("Content-Length", 0))
        body = self.rfile.read(content_length)

        event_type = self.headers.get("X-GitHub-Event", "unknown")
        delivery_id = self.headers.get("X-GitHub-Delivery", "no-id")
        signature = self.headers.get("X-Hub-Signature-256", "(none)")

        # Parse to extract action for filename
        try:
            payload = json.loads(body)
            action = payload.get("action", "no-action")
        except json.JSONDecodeError:
            action = "invalid-json"
            payload = None

        timestamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
        filename = f"{event_type}_{action}_{timestamp}.json"
        filepath = os.path.join(CAPTURE_DIR, filename)

        # Save pretty-printed payload
        os.makedirs(CAPTURE_DIR, exist_ok=True)
        with open(filepath, "w") as f:
            if payload is not None:
                json.dump(payload, f, indent=2)
            else:
                f.write(body.decode("utf-8", errors="replace"))

        # Save headers alongside
        headers_path = filepath.replace(".json", ".headers.json")
        headers_dict = {
            "X-GitHub-Event": event_type,
            "X-GitHub-Delivery": delivery_id,
            "X-Hub-Signature-256": signature,
            "Content-Type": self.headers.get("Content-Type", ""),
        }
        with open(headers_path, "w") as f:
            json.dump(headers_dict, f, indent=2)

        print(f"  Captured: {filename} ({len(body)} bytes)")
        print(f"    Event: {event_type}, Action: {action}, Delivery: {delivery_id[:8]}...")

        self.send_response(200)
        self.send_header("Content-Type", "text/plain")
        self.end_headers()
        self.wfile.write(b"OK")

    def log_message(self, format, *args):
        # Suppress default access log noise
        pass


def main():
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 9876
    os.makedirs(CAPTURE_DIR, exist_ok=True)

    server = HTTPServer(("127.0.0.1", port), WebhookHandler)
    print(f"Webhook capture server listening on http://127.0.0.1:{port}")
    print(f"Saving to: {os.path.abspath(CAPTURE_DIR)}")
    print()
    print("Next steps:")
    print(f"  1. In another terminal: gh webhook forward --events=workflow_run,workflow_job --url=http://localhost:{port}/")
    print("  2. Trigger a CI run (push, open a PR, etc.)")
    print()
    print("Press Ctrl+C to stop.")
    print()

    try:
        server.serve_forever()
    except KeyboardInterrupt:
        print("\nStopped.")
        captured = os.listdir(CAPTURE_DIR)
        payload_files = [f for f in captured if not f.endswith(".headers.json")]
        print(f"Captured {len(payload_files)} webhook payloads in {CAPTURE_DIR}/")


if __name__ == "__main__":
    main()
