"""Stands in for the VibeCafé server in the chain test (CI only): answers
GET /api/usage with made-up buckets for today and logs every request (path,
whether an Authorization header came, which process connected). Binds
127.0.0.1 only; the app reaches it as http://vibecafe-fake.test:<port>."""
import argparse
import json
from datetime import datetime, timedelta, timezone
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlsplit

from netlog import Log, owner

parser = argparse.ArgumentParser()
parser.add_argument('--port', type=int, default=18080)
parser.add_argument('--log', required=True)
args = parser.parse_args()
log = Log(args.log)


def usage():
    # Today only (the runner's clock is UTC, as the panel's "today" there), so the
    # panel's total is exactly $12.34 and 4.2M tokens whatever the hour.
    now = datetime.now(timezone.utc).replace(minute=0, second=0, microsecond=0)
    midnight = now.replace(hour=0)
    mix = [('claude-code', 'claude-opus-4-5', 0.45), ('codex', 'gpt-5.1-codex', 0.35), ('cursor', 'composer-1', 0.20)]
    buckets = []
    for hours_back, weight in ((2, 0.3), (1, 0.3), (0, 0.4)):
        start = max(now - timedelta(hours=hours_back), midnight).strftime('%Y-%m-%dT%H:%M:%SZ')
        for source, model, share in mix:
            tokens = int(4_200_000 * weight * share)
            buckets.append({
                'source': source, 'model': model, 'project': 'chain-test', 'hostname': 'ci-runner',
                'bucketStart': start, 'inputTokens': tokens // 4, 'outputTokens': tokens // 20,
                'cachedInputTokens': tokens - tokens // 4 - tokens // 20, 'reasoningOutputTokens': 0,
                'totalTokens': tokens, 'estimatedCost': round(12.34 * weight * share, 4),
            })
    return {'buckets': buckets, 'hasAnyData': True}


class Handler(BaseHTTPRequestHandler):
    protocol_version = 'HTTP/1.1'

    def _record(self):
        pid, name = owner(self.client_address, self.server.server_address)
        log.write(pid=pid, process=name, method=self.command, path=urlsplit(self.path).path,
                  host=self.headers.get('Host'), authorization=bool(self.headers.get('Authorization')))

    def _send(self, status, body):
        data = json.dumps(body).encode()
        self.send_response(status)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        self._record()
        if urlsplit(self.path).path == '/api/usage':
            self._send(200, usage())
        else:
            self._send(404, {'error': 'not found'})

    def do_POST(self):
        length = int(self.headers.get('Content-Length') or 0)
        if length:
            self.rfile.read(length)
        self._record()
        self._send(404, {'error': 'not found'})

    def log_message(self, *_):
        pass


ThreadingHTTPServer(('127.0.0.1', args.port), Handler).serve_forever()
