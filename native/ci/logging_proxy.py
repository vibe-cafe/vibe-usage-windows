"""The chain test's HTTP proxy (CI only), standing in for the user's proxy
client. Logs every request with the process that sent it. It reaches nothing
outside this machine: *.test goes to the local fake service, hosts given with
--tunnel host=port are tunnelled to that local port (the local HTTPS service),
hosts given with --hold are answered 200 and then held open without
forwarding (a managed Node stays waiting, so quitting with Node alive can be
tested), everything else is refused with 403."""
import argparse
import select
import socket
import socketserver
from urllib.parse import urlsplit

from netlog import Log, owner

parser = argparse.ArgumentParser()
parser.add_argument('--port', type=int, default=18888)
parser.add_argument('--log', required=True)
parser.add_argument('--hold', action='append', default=[])
parser.add_argument('--tunnel', action='append', default=[])
args = parser.parse_args()
tunnels = {item.split('=')[0]: int(item.split('=')[1]) for item in args.tunnel}
log = Log(args.log)
REFUSED = b'HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n'


def pipe(a, b):
    sockets = [a, b]
    try:
        while True:
            ready, _, _ = select.select(sockets, [], [], 60)
            if not ready:
                return
            for s in ready:
                data = s.recv(65536)
                if not data:
                    return
                (b if s is a else a).sendall(data)
    except OSError:
        return
    finally:
        b.close()


def hold(c):
    c.settimeout(600)
    try:
        while c.recv(65536):
            pass
    except OSError:
        pass


class Handler(socketserver.BaseRequestHandler):
    def handle(self):
        c = self.request
        c.settimeout(30)
        head = b''
        while b'\r\n\r\n' not in head and len(head) < 65536:
            chunk = c.recv(4096)
            if not chunk:
                return
            head += chunk
        header, _, rest = head.partition(b'\r\n\r\n')
        lines = header.decode('latin-1').split('\r\n')
        parts = lines[0].split(' ')
        method, target = (parts + ['', ''])[:2]
        pid, name = owner(self.client_address, c.getsockname())
        if method == 'CONNECT':
            host, _, port = target.rpartition(':')
            port = int(port or 443)
            if host.endswith('.test'):
                verdict, local = 'tunnel-to-fake', port
            elif host in tunnels:
                verdict, local = 'tunnel-to-local-https', tunnels[host]
            else:
                verdict, local = ('held-open' if host in args.hold else 'refused-403'), None
            log.write(pid=pid, process=name, method=method, host=host, port=port, verdict=verdict)
            if verdict == 'refused-403':
                c.sendall(REFUSED)
            elif verdict == 'held-open':
                c.sendall(b'HTTP/1.1 200 Connection established\r\n\r\n')
                hold(c)
            else:
                upstream = socket.create_connection(('127.0.0.1', local))
                c.sendall(b'HTTP/1.1 200 Connection established\r\n\r\n')
                if rest:
                    upstream.sendall(rest)
                pipe(c, upstream)
            return
        url = urlsplit(target)
        host, port = url.hostname or '', url.port or 80
        verdict = 'forwarded-to-fake' if host.endswith('.test') else 'refused-403'
        log.write(pid=pid, process=name, method=method, host=host, port=port, path=url.path, verdict=verdict)
        if verdict == 'refused-403':
            c.sendall(REFUSED)
            return
        upstream = socket.create_connection(('127.0.0.1', port))
        path = url.path + ('?' + url.query if url.query else '')
        kept = [l for l in lines[1:] if not l.lower().startswith(('proxy-', 'connection:'))]
        upstream.sendall(('\r\n'.join([f'{method} {path} HTTP/1.1', *kept, 'Connection: close']) + '\r\n\r\n').encode('latin-1') + rest)
        pipe(c, upstream)


class Server(socketserver.ThreadingTCPServer):
    daemon_threads = True
    allow_reuse_address = True


server = Server(('127.0.0.1', args.port), Handler)
# The moment the port accepts connections: the test's "proxy ready" time.
log.write(event='listening', port=args.port)
server.serve_forever()
