"""The chain test's local HTTPS service (CI only). It stands in for the HTTPS
hosts under test (chatgpt.com, github.com, registry.npmjs.org) with
certificates from a throwaway test CA the runner trusts, so it sees each
request's method and path, and whether it carried an Authorization header, a
ChatGPT-Account-Id header or a cookie (presence only, never the value). It is
reached directly (the hosts file points those names at 127.0.0.1) or through
the test proxy's tunnel. Its answers keep everything local: chatgpt.com's
/cdn-cgi/trace has no usable country (loc=XX, so the egress check stays
unconfirmed), /backend-api/* is 401, everything else 404. A client that does
not trust the test CA (Node keeps its own CA list) is logged as a failed
handshake with its server name."""
import argparse
import os
import socketserver
import ssl

from netlog import Log, owner

parser = argparse.ArgumentParser()
parser.add_argument('--port', type=int, default=443)
parser.add_argument('--log', required=True)
parser.add_argument('--certs', required=True)
parser.add_argument('--host', action='append', default=[])
args = parser.parse_args()
log = Log(args.log)

contexts = {}
for host in args.host:
    ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    ctx.load_cert_chain(os.path.join(args.certs, host + '.crt'), os.path.join(args.certs, host + '.key'))
    contexts[host] = ctx
default = contexts[args.host[0]]
seen = {}


def choose(sock, server_name, _ctx):
    seen[id(sock)] = server_name
    if server_name in contexts:
        sock.context = contexts[server_name]


default.sni_callback = choose
TRACE = b'fl=1\nh=chatgpt.com\nip=127.0.0.1\nts=0\nvisit_scheme=https\nloc=XX\ntls=TLSv1.3\nwarp=off\n'


def answer(status, body, kind):
    return (f'HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {len(body)}\r\nConnection: close\r\n\r\n').encode() + body


class Handler(socketserver.BaseRequestHandler):
    def handle(self):
        raw = self.request
        pid, name = owner(self.client_address, raw.getsockname())
        raw.settimeout(5)
        tls = default.wrap_socket(raw, server_side=True, do_handshake_on_connect=False)
        try:
            tls.do_handshake()
        except (ssl.SSLError, OSError) as error:
            log.write(pid=pid, process=name, sni=seen.pop(id(tls), None), handshake='failed', error=type(error).__name__)
            return
        sni = seen.pop(id(tls), None)
        head = b''
        try:
            while b'\r\n\r\n' not in head and len(head) < 65536:
                chunk = tls.recv(4096)
                if not chunk:
                    break
                head += chunk
        except (ssl.SSLError, OSError):
            pass
        lines = head.split(b'\r\n\r\n')[0].decode('latin-1').split('\r\n')
        method, path = (lines[0].split(' ') + ['', ''])[:2]
        headers = {l.split(':', 1)[0].strip().lower() for l in lines[1:] if ':' in l}
        log.write(pid=pid, process=name, sni=sni, handshake='ok', method=method, path=path.split('?')[0],
                  authorization='authorization' in headers, account='chatgpt-account-id' in headers, cookie='cookie' in headers)
        if sni == 'chatgpt.com' and path.startswith('/cdn-cgi/trace'):
            reply = answer('200 OK', TRACE, 'text/plain')
        elif path.startswith('/backend-api/'):
            reply = answer('401 Unauthorized', b'{}', 'application/json')
        else:
            reply = answer('404 Not Found', b'', 'text/plain')
        try:
            tls.sendall(reply)
            tls.close()
        except (ssl.SSLError, OSError):
            pass


class Server(socketserver.ThreadingTCPServer):
    daemon_threads = True
    allow_reuse_address = True


Server(('127.0.0.1', args.port), Handler).serve_forever()
