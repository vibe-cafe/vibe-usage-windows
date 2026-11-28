"""Catches direct HTTPS attempts in the chain test (CI only). The hosts file
points the hosts under test (chatgpt.com, registry.npmjs.org, github.com) at
127.0.0.1; this listener on 127.0.0.1:443 reads the TLS ClientHello, logs the
server name and the process, and closes without answering."""
import argparse
import socketserver

from netlog import Log, owner

parser = argparse.ArgumentParser()
parser.add_argument('--port', type=int, default=443)
parser.add_argument('--log', required=True)
args = parser.parse_args()
log = Log(args.log)


def server_name(data):
    try:
        if len(data) < 6 or data[0] != 0x16 or data[5] != 1:
            return None
        p = 5 + 4 + 2 + 32
        p += 1 + data[p]
        p += 2 + int.from_bytes(data[p:p + 2], 'big')
        p += 1 + data[p]
        end = p + 2 + int.from_bytes(data[p:p + 2], 'big')
        p += 2
        while p + 4 <= end:
            kind, length = int.from_bytes(data[p:p + 2], 'big'), int.from_bytes(data[p + 2:p + 4], 'big')
            p += 4
            if kind == 0:
                size = int.from_bytes(data[p + 3:p + 5], 'big')
                return data[p + 5:p + 5 + size].decode('ascii', 'replace')
            p += length
    except (IndexError, ValueError):
        return None
    return None


class Handler(socketserver.BaseRequestHandler):
    def handle(self):
        c = self.request
        pid, name = owner(self.client_address, c.getsockname())
        c.settimeout(3)
        try:
            data = c.recv(4096)
        except OSError:
            data = b''
        log.write(pid=pid, process=name, sni=server_name(data))


class Server(socketserver.ThreadingTCPServer):
    daemon_threads = True
    allow_reuse_address = True


Server(('127.0.0.1', args.port), Handler).serve_forever()
